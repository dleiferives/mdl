use std::env;
use std::path::PathBuf;

use mdl_compiler::analysis::minecraft::{
    AnalysisArithmeticCaps, CommandLimitStatus, NoFiniteBoundReason, TargetExecutionAnalysisLimits,
};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{
    CompilationOptions, CompilationOutput, FrontendLimits, FunctionVisibility, ImportName,
    ModuleDependency, ModuleInput, ModuleKey, PackageInput, SourceFunctionId, SourceInput,
    compile_package,
};
use mdl_compiler::ir::core::{CoreEvaluationLimits, CoreEvaluator, CoreValue};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::{ServerConfig, ServerSandbox, TestServer, sha256_file};
use serde::Deserialize;

const OPCODE: &str = include_str!("../../../tests/programs/brainfuck/src/opcode.mdl");
const BYTE: &str = include_str!("../../../tests/programs/brainfuck/src/byte.mdl");
const TAPE: &str = include_str!("../../../tests/programs/brainfuck/src/tape.mdl");
const IO: &str = include_str!("../../../tests/programs/brainfuck/src/io.mdl");
const PARSER: &str = include_str!("../../../tests/programs/brainfuck/src/parser.mdl");
const INTERPRETER: &str = include_str!("../../../tests/programs/brainfuck/src/interpreter.mdl");
const MAIN: &str = include_str!("../../../tests/programs/brainfuck/src/main.mdl");
const CASES: &str = include_str!("../../../tests/programs/brainfuck/cases.json");
const BOOK_CASES: &str = include_str!("../../../tests/programs/brainfuck/book-cases.json");
const SERVER_SHA256: &str = "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";
const EXTRACTED_SERVER_SHA256: &str =
    "183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    tier: String,
    program: String,
    input: Vec<i32>,
    fuel: i32,
    max_units: i32,
    max_instructions: i32,
    max_input: i32,
    max_output: i32,
    status: i32,
    error_position: i32,
    output: Vec<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BookCase {
    name: String,
    item: String,
    pages: Vec<String>,
    expected_marker: String,
    all_policies: bool,
}

#[derive(Debug, Eq, PartialEq)]
struct ReferenceResult {
    left: Vec<i32>,
    right: Vec<i32>,
    past: Vec<i32>,
    future: Vec<i32>,
    input: Vec<i32>,
    output: Vec<i32>,
    remaining_fuel: i32,
    dispatches: i32,
    status: i32,
    position: i32,
}

#[test]
fn brainfuck_package_compiles_and_showcase_evaluates() {
    for core in [CoreOptimizationLevel::None, CoreOptimizationLevel::Baseline] {
        for minecraft in [
            MinecraftOptimizationLevel::None,
            MinecraftOptimizationLevel::Baseline,
        ] {
            let output = compile_brainfuck(core, minecraft).unwrap();
            let showcase = source_function(&output, "main", "showcase_checksum");
            let core_function = output.source_to_core().function(showcase).unwrap();
            let result = CoreEvaluator::for_verified_program(
                output.core_optimization().program(),
                CoreEvaluationLimits::new(5_000_000, 4_096),
            )
            .evaluate(core_function, &[CoreValue::I32(256)])
            .unwrap();
            let run_text = source_function(&output, "interpreter", "run_text");
            let run_core = output.source_to_core().function(run_text).unwrap();
            let run = CoreEvaluator::for_verified_program(
                output.core_optimization().program(),
                CoreEvaluationLimits::new(5_000_000, 4_096),
            )
            .evaluate(
                run_core,
                &[
                    CoreValue::string("++++++++[>++++++++<-]>+."),
                    CoreValue::list_i32(Vec::<i32>::new()),
                    CoreValue::I32(256),
                    CoreValue::I32(8192),
                    CoreValue::I32(8192),
                    CoreValue::I32(4096),
                    CoreValue::I32(4096),
                ],
            )
            .unwrap();
            assert_eq!(
                run.results()[5..],
                [
                    CoreValue::list_i32(vec![65]),
                    CoreValue::I32(148),
                    CoreValue::I32(108),
                    CoreValue::I32(0),
                    CoreValue::I32(24),
                ]
            );
            assert_eq!(result.results(), &[CoreValue::I32(386)]);
        }
    }
}

#[test]
fn opcode_byte_tape_parser_and_io_boundaries_are_independently_observable() {
    let output = compile_brainfuck(
        CoreOptimizationLevel::Baseline,
        MinecraftOptimizationLevel::Baseline,
    )
    .unwrap();
    let evaluator = CoreEvaluator::for_verified_program(
        output.core_optimization().program(),
        CoreEvaluationLimits::new(1_000_000, 4_096),
    );
    let evaluate = |module, name, arguments: &[CoreValue]| {
        let source = source_function(&output, module, name);
        let function = output.source_to_core().function(source).unwrap();
        evaluator.evaluate(function, arguments).unwrap()
    };

    assert_eq!(
        evaluate("byte", "increment", &[CoreValue::I32(255)]).results(),
        &[CoreValue::I32(0)]
    );
    assert_eq!(
        evaluate("byte", "decrement", &[CoreValue::I32(0)]).results(),
        &[CoreValue::I32(255)]
    );
    assert_eq!(
        evaluate(
            "tape",
            "right_left",
            &[
                CoreValue::list_i32(vec![7]),
                CoreValue::list_i32(vec![9, 3]),
            ],
        )
        .results(),
        &[CoreValue::list_i32(vec![7, 3])]
    );
    assert_eq!(
        evaluate("tape", "right_right", &[CoreValue::list_i32(vec![9, 3])],).results(),
        &[CoreValue::list_i32(vec![9])]
    );
    assert_eq!(
        evaluate(
            "tape",
            "left_right",
            &[
                CoreValue::list_i32(vec![7, 3]),
                CoreValue::list_i32(vec![9]),
            ],
        )
        .results(),
        &[CoreValue::list_i32(vec![9, 3])]
    );
    assert_eq!(
        evaluate(
            "parser",
            "append_normalized",
            &[
                CoreValue::string("ignored😀+[-]"),
                CoreValue::list_i32(Vec::<i32>::new()),
            ],
        )
        .results(),
        &[CoreValue::list_i32(vec![93, 45, 91, 43])]
    );
    assert_eq!(
        evaluate(
            "parser",
            "bracket_validation",
            &[CoreValue::list_i32(vec![93, 43, 91])],
        )
        .results(),
        &[CoreValue::list_i32(vec![0, 0])]
    );
    assert_eq!(
        evaluate("io", "read_or_zero", &[CoreValue::list_i32(vec![4, 8])],).results(),
        &[CoreValue::I32(8)]
    );
}

#[test]
fn package_boundary_and_book_case_schema_are_complete() {
    let sources = [OPCODE, BYTE, TAPE, IO, PARSER, INTERPRETER, MAIN];
    assert!(
        sources
            .iter()
            .all(|source| !source.contains("unsafe minecraft"))
    );
    let page_call = "components.\"minecraft:written_book_content\".pages[";
    assert_eq!(INTERPRETER.matches(page_call).count(), 100);
    for page in 0..100 {
        assert!(
            INTERPRETER.contains(&format!("{page_call}{page}].raw")),
            "missing static page {page}"
        );
    }
    let book_cases: Vec<BookCase> = serde_json::from_str(BOOK_CASES).unwrap();
    assert_eq!(book_cases.len(), 3);

    let output = compile_brainfuck(
        CoreOptimizationLevel::Baseline,
        MinecraftOptimizationLevel::Baseline,
    )
    .unwrap();
    assert_eq!(output.checked_frontend().module_count(), 7);
    assert!(
        output
            .checked_frontend()
            .function_ids()
            .all(|function| output.source_to_core().function(function).is_some())
    );
}

#[test]
// Keep the four exact products and every reconciled metric in one audit table.
#[allow(clippy::too_many_lines)]
fn capstone_footprint_recipe_and_cost_evidence_is_pinned() {
    struct Expected {
        label: &'static str,
        core: CoreOptimizationLevel,
        minecraft: MinecraftOptimizationLevel,
        files: usize,
        functions: usize,
        lines: usize,
        bytes: usize,
        max_line: usize,
        command_nodes: usize,
        score_commands: usize,
        data_commands: usize,
        homes: usize,
        realizations: usize,
        materializations: usize,
        physical_recipe_sequence: usize,
    }
    let expected = [
        Expected {
            label: "none-none",
            core: CoreOptimizationLevel::None,
            minecraft: MinecraftOptimizationLevel::None,
            files: 228,
            functions: 226,
            lines: 2_610,
            bytes: 211_996,
            max_line: 163,
            command_nodes: 3_158,
            score_commands: 1_025,
            data_commands: 1_144,
            homes: 1_156,
            realizations: 1_114,
            materializations: 136,
            physical_recipe_sequence: 136,
        },
        Expected {
            label: "none-baseline",
            core: CoreOptimizationLevel::None,
            minecraft: MinecraftOptimizationLevel::Baseline,
            files: 221,
            functions: 219,
            lines: 1_961,
            bytes: 156_837,
            max_line: 161,
            command_nodes: 2_502,
            score_commands: 653,
            data_commands: 874,
            homes: 826,
            realizations: 991,
            materializations: 134,
            physical_recipe_sequence: 134,
        },
        Expected {
            label: "baseline-none",
            core: CoreOptimizationLevel::Baseline,
            minecraft: MinecraftOptimizationLevel::None,
            files: 208,
            functions: 206,
            lines: 2_528,
            bytes: 208_168,
            max_line: 163,
            command_nodes: 3_056,
            score_commands: 966,
            data_commands: 1_141,
            homes: 1_092,
            realizations: 1_050,
            materializations: 76,
            physical_recipe_sequence: 76,
        },
        Expected {
            label: "baseline-baseline",
            core: CoreOptimizationLevel::Baseline,
            minecraft: MinecraftOptimizationLevel::Baseline,
            files: 202,
            functions: 200,
            lines: 1_888,
            bytes: 153_716,
            max_line: 161,
            command_nodes: 2_410,
            score_commands: 599,
            data_commands: 874,
            homes: 772,
            realizations: 924,
            materializations: 76,
            physical_recipe_sequence: 76,
        },
    ];
    for expected in expected {
        let output = compile_brainfuck(expected.core, expected.minecraft).unwrap();
        let footprint = output.emission().footprint();
        assert_eq!(
            footprint.files().len(),
            expected.files,
            "{} files",
            expected.label
        );
        assert_eq!(footprint.metadata_files(), 1, "{} metadata", expected.label);
        assert_eq!(
            footprint.function_files(),
            expected.functions,
            "{} functions",
            expected.label
        );
        assert_eq!(footprint.function_tag_files(), 1, "{} tags", expected.label);
        assert_eq!(
            footprint.physical_function_lines(),
            expected.lines,
            "{} lines",
            expected.label
        );
        assert_eq!(
            footprint.total_utf8_bytes(),
            expected.bytes,
            "{} bytes",
            expected.label
        );
        assert_eq!(
            footprint.maximum_function_line_utf16_units(),
            expected.max_line,
            "{} max line",
            expected.label
        );
        assert_eq!(
            footprint.trace_records(),
            expected.lines,
            "{} trace",
            expected.label
        );

        let statistics = output.lowering().report().statistics();
        assert_eq!(
            statistics.homes(),
            expected.homes,
            "{} homes",
            expected.label
        );
        assert_eq!(
            statistics.physical_storages(),
            expected.homes,
            "{} storage",
            expected.label
        );
        assert_eq!(
            statistics.realizations(),
            expected.realizations,
            "{} realizations",
            expected.label
        );
        assert_eq!(
            statistics.materializations(),
            expected.materializations,
            "{} materializations",
            expected.label
        );
        assert_eq!(
            statistics.physical_recipe_sequence_operations(),
            expected.physical_recipe_sequence,
            "{} recipes",
            expected.label
        );
        assert_eq!(
            statistics.physical_recipe_forks(),
            0,
            "{} recipe forks",
            expected.label
        );
        assert_eq!(
            statistics.recursive_call_occurrences(),
            0,
            "{} recursive calls",
            expected.label
        );

        let report = output.target_analysis().as_ref().unwrap();
        let census = report.census();
        assert_eq!(
            census.functions(),
            expected.functions,
            "{} census functions",
            expected.label
        );
        assert_eq!(
            census.command_nodes(),
            expected.command_nodes,
            "{} command nodes",
            expected.label
        );
        assert_eq!(
            census.score_commands(),
            expected.score_commands,
            "{} scores",
            expected.label
        );
        assert_eq!(
            census.data_commands(),
            expected.data_commands,
            "{} data",
            expected.label
        );
        assert_eq!(census.say_commands(), 7, "{} say", expected.label);
        assert_eq!(census.raw_commands(), 0, "{} raw", expected.label);
        assert_eq!(report.roots().len(), 4, "{} roots", expected.label);
        assert_eq!(
            report
                .roots()
                .iter()
                .filter(|root| matches!(
                    root.sequence_limit_status(),
                    CommandLimitStatus::NoFiniteBoundProven(NoFiniteBoundReason::PositiveCycle)
                ))
                .count(),
            3,
            "{} cyclic roots",
            expected.label,
        );
        assert!(
            report
                .roots()
                .iter()
                .all(|root| root.fork_limit_status() == CommandLimitStatus::ProvenWithin),
            "{} fork proof",
            expected.label
        );

        let pack = output
            .emission()
            .pack()
            .files()
            .iter()
            .filter_map(|file| std::str::from_utf8(file.bytes()).ok())
            .collect::<String>();
        assert!(!pack.lines().any(|line| line.starts_with('$')));
        assert!(!pack.contains("unsafe minecraft"));
        assert!(pack.contains("written_book_content"));
        assert!(pack.contains("MDL_PS3_BRAINFUCK_A"));
        assert!(pack.contains(" append from storage "));
        assert!(pack.contains(" set string storage "));
    }
}

#[test]
fn frozen_corpus_matches_an_independent_reference_for_every_policy() {
    let cases: Vec<Case> = serde_json::from_str(CASES).unwrap();
    assert!(
        cases.len() >= 22,
        "the diagnostic corpus unexpectedly shrank"
    );
    for core in [CoreOptimizationLevel::None, CoreOptimizationLevel::Baseline] {
        for minecraft in [
            MinecraftOptimizationLevel::None,
            MinecraftOptimizationLevel::Baseline,
        ] {
            let output = compile_brainfuck(core, minecraft).unwrap();
            let source = source_function(&output, "interpreter", "run_text");
            let function = output.source_to_core().function(source).unwrap();
            let evaluator = CoreEvaluator::for_verified_program(
                output.core_optimization().program(),
                CoreEvaluationLimits::new(5_000_000, 16_384),
            );
            for case in &cases {
                assert!(
                    matches!(
                        case.tier.as_str(),
                        "parser" | "interpreter" | "limits" | "showcase"
                    ),
                    "{} has an unknown tier",
                    case.name,
                );
                let expected = reference(case);
                assert_eq!(expected.status, case.status, "{} corpus status", case.name);
                assert_eq!(
                    expected.position, case.error_position,
                    "{} corpus position",
                    case.name,
                );
                assert_eq!(expected.output, case.output, "{} corpus output", case.name);
                let actual = evaluator
                    .evaluate(
                        function,
                        &[
                            CoreValue::string(&case.program),
                            CoreValue::list_i32(case.input.clone()),
                            CoreValue::I32(case.fuel),
                            CoreValue::I32(case.max_units),
                            CoreValue::I32(case.max_instructions),
                            CoreValue::I32(case.max_input),
                            CoreValue::I32(case.max_output),
                        ],
                    )
                    .unwrap_or_else(|error| panic!("{}: {error}", case.name));
                assert_run_result(actual.results(), &expected, &case.name);
            }
        }
    }
}

#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn split_written_book_executes_the_capstone_under_all_four_policies() {
    if let Err(error) = run_book_server_test() {
        panic!("{error}");
    }
}

fn run_book_server_test() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let hash = sha256_file(&server_jar).map_err(|error| error.to_string())?;
    if ![SERVER_SHA256, EXTRACTED_SERVER_SHA256].contains(&hash.as_str()) {
        return Err(format!("expected the pinned Java 26.2 server, got {hash}"));
    }
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let policies = [
        (
            CoreOptimizationLevel::None,
            MinecraftOptimizationLevel::None,
        ),
        (
            CoreOptimizationLevel::None,
            MinecraftOptimizationLevel::Baseline,
        ),
        (
            CoreOptimizationLevel::Baseline,
            MinecraftOptimizationLevel::None,
        ),
        (
            CoreOptimizationLevel::Baseline,
            MinecraftOptimizationLevel::Baseline,
        ),
    ];
    let mut compiled = Vec::new();
    for (index, (core, minecraft)) in policies.into_iter().enumerate() {
        let namespace = format!("mdl_ps3_bf_{index}");
        let objective = format!("mps3.bf{index}");
        let output = compile_brainfuck_named(core, minecraft, &namespace, &objective)
            .map_err(|error| format!("policy {index}: {error}"))?;
        let source = source_function(&output, "main", "run_book_showcase");
        let entry = output
            .source_function_abi(source)
            .ok_or_else(|| format!("policy {index} has no book ABI"))?
            .entry_resource()
            .to_string();
        compiled.push((namespace, output, entry));
    }

    let sandbox = ServerSandbox::create(env::var_os("MDL_KEEP_TEST_DIR").is_some())
        .map_err(|error| error.to_string())?;
    for (namespace, output, _) in &compiled {
        sandbox
            .install_datapack(
                namespace,
                output
                    .emission()
                    .pack()
                    .files()
                    .iter()
                    .map(|file| (file.path().as_str(), file.bytes())),
            )
            .map_err(|error| error.to_string())?;
    }
    let mut server = sandbox
        .start(&ServerConfig::new(java, server_jar))
        .map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    if let Err(error) = exercise_book_server(&mut server, &compiled) {
        server.preserve_sandbox();
        return Err(format!(
            "{error}\nPS-3 Brainfuck sandbox preserved at {}",
            root.display()
        ));
    }
    server.shutdown().map_err(|error| error.to_string())
}

fn exercise_book_server(
    server: &mut TestServer,
    compiled: &[(String, CompilationOutput, String)],
) -> Result<(), String> {
    let cases: Vec<BookCase> =
        serde_json::from_str(BOOK_CASES).map_err(|error| error.to_string())?;
    let showcase = cases
        .iter()
        .find(|case| case.name == "split-showcase-a")
        .ok_or_else(|| "missing split-showcase-a book case".to_owned())?;
    if showcase.item != "written_book" || !showcase.all_policies {
        return Err("split-showcase-a has the wrong adapter policy".to_owned());
    }
    let wrong_item = cases
        .iter()
        .find(|case| case.name == "wrong-item")
        .ok_or_else(|| "missing wrong-item book case".to_owned())?;
    let malformed = cases
        .iter()
        .find(|case| case.name == "malformed-bracket")
        .ok_or_else(|| "missing malformed-bracket book case".to_owned())?;
    command_wait(
        server,
        "execute in minecraft:overworld run forceload add 0 0",
        "force loaded",
    )?;
    summon_reader(server, &pages_snbt(&showcase.pages))?;

    for (index, (_, _, entry)) in compiled.iter().enumerate() {
        let checkpoint = server.log_checkpoint();
        server
            .command(&format!("function {entry}"))
            .map_err(|error| error.to_string())?;
        let barrier = format!("MDL_PS3_A_BARRIER_{index}");
        command_wait(server, &format!("say {barrier}"), &barrier)?;
        let lines = server
            .matching_log_lines_since(checkpoint, &showcase.expected_marker)
            .map_err(|error| error.to_string())?;
        if lines.len() != 1 || !lines[0].contains("MDL_PS3_READER") {
            return Err(format!(
                "policy {index} did not produce exactly one reader-attributed A: {lines:?}"
            ));
        }
    }

    command_wait(
        server,
        &format!(
            "item replace entity @e[type=minecraft:armor_stand,tag=mdl_ps3_book,limit=1] weapon.mainhand with minecraft:{}",
            wrong_item.item
        ),
        "Replaced",
    )?;
    observe_marker(
        server,
        &compiled[0].2,
        &wrong_item.expected_marker,
        "NO_PROGRAM",
    )?;

    command_wait(
        server,
        "kill @e[type=minecraft:armor_stand,tag=mdl_ps3_book]",
        "Killed",
    )?;
    summon_reader(server, &pages_snbt(&malformed.pages))?;
    observe_marker(
        server,
        &compiled[0].2,
        &malformed.expected_marker,
        "PARSE_ERROR",
    )?;
    Ok(())
}

fn pages_snbt(pages: &[String]) -> String {
    format!(
        "[{}]",
        pages
            .iter()
            .map(|page| format!("{{raw:{{text:'{page}'}}}}"))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn summon_reader(server: &mut TestServer, pages: &str) -> Result<(), String> {
    command_wait(
        server,
        concat!(
            "execute in minecraft:overworld run summon minecraft:armor_stand 0 5 0 ",
            "{Tags:['mdl_ps3_book'],CustomName:'\"MDL_PS3_READER\"',NoGravity:1b}"
        ),
        "Summoned new \"MDL_PS3_READER\"",
    )?;
    command_wait(
        server,
        &format!(
            concat!(
                "data merge entity @e[type=minecraft:armor_stand,tag=mdl_ps3_book,limit=1] ",
                "{{equipment:{{mainhand:{{id:'minecraft:written_book',count:1,components:",
                "{{'minecraft:written_book_content':{{pages:{pages},title:{{raw:'PS3'}},",
                "author:'mdl',generation:0,resolved:true}}}}}}}}}}"
            ),
            pages = pages,
        ),
        "Modified entity data",
    )
}

fn observe_marker(
    server: &mut TestServer,
    entry: &str,
    marker: &str,
    barrier_suffix: &str,
) -> Result<(), String> {
    let checkpoint = server.log_checkpoint();
    server
        .command(&format!("function {entry}"))
        .map_err(|error| error.to_string())?;
    let barrier = format!("MDL_PS3_{barrier_suffix}_BARRIER");
    command_wait(server, &format!("say {barrier}"), &barrier)?;
    let lines = server
        .matching_log_lines_since(checkpoint, marker)
        .map_err(|error| error.to_string())?;
    if lines.len() == 1 && lines[0].contains("MDL_PS3_READER") {
        Ok(())
    } else {
        Err(format!(
            "expected one reader-attributed {marker}: {lines:?}"
        ))
    }
}

fn command_wait(server: &mut TestServer, command: &str, expected: &str) -> Result<(), String> {
    server.command(command).map_err(|error| error.to_string())?;
    server
        .wait_for_command_log(expected)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn assert_run_result(actual: &[CoreValue], expected: &ReferenceResult, case: &str) {
    assert_eq!(
        actual,
        &[
            CoreValue::list_i32(expected.left.clone()),
            CoreValue::list_i32(expected.right.clone()),
            CoreValue::list_i32(expected.past.clone()),
            CoreValue::list_i32(expected.future.clone()),
            CoreValue::list_i32(expected.input.clone()),
            CoreValue::list_i32(expected.output.clone()),
            CoreValue::I32(expected.remaining_fuel),
            CoreValue::I32(expected.dispatches),
            CoreValue::I32(expected.status),
            CoreValue::I32(expected.position),
        ],
        "{case}",
    );
}

// This intentionally remains one plainly auditable state machine independent from
// the MDL module decomposition it checks.
#[allow(clippy::too_many_lines)]
fn reference(case: &Case) -> ReferenceResult {
    let mut program = Vec::new();
    let units = case.program.encode_utf16().collect::<Vec<_>>();
    let mut status = 0;
    let mut position = 0;
    if case.max_units < 0 || units.len() > usize::try_from(case.max_units).unwrap_or(0) {
        status = 4;
    } else if case.max_instructions < 0 {
        status = 5;
    } else {
        program = units
            .iter()
            .rev()
            .filter_map(|unit| match *unit {
                62 => Some(62),
                60 => Some(60),
                43 => Some(43),
                45 => Some(45),
                46 => Some(46),
                44 => Some(44),
                91 => Some(91),
                93 => Some(93),
                _ => None,
            })
            .collect();
        if program.len() > usize::try_from(case.max_instructions).unwrap_or(0) {
            status = 5;
            position = case.max_instructions;
        } else if let Some((bracket_status, bracket_position)) = invalid_bracket(&program) {
            status = bracket_status;
            position = bracket_position;
        }
    }

    let mut result = ReferenceResult {
        left: Vec::new(),
        right: vec![0],
        past: Vec::new(),
        future: program,
        input: case.input.clone(),
        output: Vec::new(),
        remaining_fuel: case.fuel,
        dispatches: 0,
        status,
        position,
    };
    if result.status == 0 {
        if case.fuel < 0 {
            result.status = 9;
        } else if case.max_input < 0
            || result.input.len() > usize::try_from(case.max_input).unwrap_or(0)
        {
            result.status = 7;
        } else if case.max_output < 0 {
            result.status = 6;
        }
    }
    if result.status == 0 && result.input.iter().any(|byte| !(0..=255).contains(byte)) {
        result.status = 8;
    }
    while result.status == 0 && !result.future.is_empty() && result.remaining_fuel > 0 {
        let operation = result.future.pop().unwrap();
        result.past.push(operation);
        result.remaining_fuel = result.remaining_fuel.wrapping_sub(1);
        result.dispatches = result.dispatches.wrapping_add(1);
        match operation {
            62 => {
                result.left.push(*result.right.last().unwrap_or(&0));
                result.right.pop();
                if result.right.is_empty() {
                    result.right.push(0);
                }
            }
            60 => {
                let value = result.left.pop().unwrap_or(0);
                result.right.push(value);
            }
            43 => {
                let current = result.right.last_mut().unwrap();
                *current = if *current == 255 {
                    0
                } else {
                    current.wrapping_add(1)
                };
            }
            45 => {
                let current = result.right.last_mut().unwrap();
                *current = if *current == 0 {
                    255
                } else {
                    current.wrapping_sub(1)
                };
            }
            46 => {
                if result.output.len() >= usize::try_from(case.max_output).unwrap_or(0) {
                    result.status = 6;
                    result.position = i32::try_from(result.past.len()).unwrap();
                } else {
                    result.output.push(*result.right.last().unwrap_or(&0));
                }
            }
            44 => {
                let value = result.input.pop().unwrap_or(0);
                *result.right.last_mut().unwrap() = value;
            }
            91 if *result.right.last().unwrap_or(&0) == 0 => {
                let mut depth = 0_i32;
                while let Some(scanned) = result.future.pop() {
                    result.past.push(scanned);
                    match scanned {
                        91 => depth += 1,
                        93 if depth == 0 => break,
                        93 => depth -= 1,
                        _ => {}
                    }
                }
            }
            93 if *result.right.last().unwrap_or(&0) != 0 => {
                result.past.pop();
                result.future.push(93);
                let mut depth = 0_i32;
                while let Some(scanned) = result.past.pop() {
                    match scanned {
                        93 => {
                            depth += 1;
                            result.future.push(scanned);
                        }
                        91 if depth == 0 => {
                            result.past.push(scanned);
                            break;
                        }
                        91 => {
                            depth -= 1;
                            result.future.push(scanned);
                        }
                        _ => result.future.push(scanned),
                    }
                }
            }
            _ => {}
        }
    }
    if result.status == 0 {
        result.position = i32::try_from(result.past.len()).unwrap();
        if !result.future.is_empty() {
            result.status = 1;
        }
    }
    result
}

fn invalid_bracket(program: &[i32]) -> Option<(i32, i32)> {
    let mut opens = Vec::new();
    for (position, opcode) in program.iter().rev().enumerate() {
        match opcode {
            91 => opens.push(i32::try_from(position).unwrap()),
            93 if opens.pop().is_none() => {
                return Some((3, i32::try_from(position).unwrap()));
            }
            _ => {}
        }
    }
    opens.last().copied().map(|position| (2, position))
}

fn compile_brainfuck(
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
) -> Result<CompilationOutput, mdl_compiler::frontend::CompilationFailure> {
    compile_brainfuck_named(core, minecraft, "mdl_ps3_bf", "mdl.ps3.bf")
}

fn compile_brainfuck_named(
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
    namespace: &str,
    objective: &str,
) -> Result<CompilationOutput, mdl_compiler::frontend::CompilationFailure> {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new(namespace).unwrap(),
        ObjectiveName::new(objective).unwrap(),
    )
    .unwrap()
    .with_optimization_level(minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    compile_package(
        brainfuck_package(),
        &CompilationOptions::new(
            FrontendLimits::DEFAULT,
            CoreOptimizationOptions::new(core),
            lowering,
            TargetExecutionAnalysisLimits::new(arithmetic, 1_000_000, 1_000_000),
            EmissionOptions::new("MDL PS-3 Brainfuck capstone"),
        ),
    )
}

fn brainfuck_package() -> PackageInput {
    let main = key("main");
    let opcode = key("opcode");
    let byte = key("byte");
    let tape = key("tape");
    let io = key("io");
    let parser = key("parser");
    let interpreter = key("interpreter");
    PackageInput::new(
        main.clone(),
        vec![
            module(main, "main.mdl", MAIN, &[("interpreter", &interpreter)]),
            module(opcode, "opcode.mdl", OPCODE, &[]),
            module(byte.clone(), "byte.mdl", BYTE, &[]),
            module(tape.clone(), "tape.mdl", TAPE, &[]),
            module(io.clone(), "io.mdl", IO, &[]),
            module(
                parser.clone(),
                "parser.mdl",
                PARSER,
                &[("opcode", &key("opcode"))],
            ),
            module(
                interpreter,
                "interpreter.mdl",
                INTERPRETER,
                &[
                    ("byte", &byte),
                    ("io", &io),
                    ("parser", &parser),
                    ("tape", &tape),
                ],
            ),
        ],
    )
}

fn module(
    key: ModuleKey,
    path: &'static str,
    source: &'static str,
    dependencies: &[(&str, &ModuleKey)],
) -> ModuleInput {
    ModuleInput::new(
        key,
        SourceInput::new(path, source),
        dependencies
            .iter()
            .map(|(name, target)| {
                ModuleDependency::new(ImportName::new(*name).unwrap(), (*target).clone())
            })
            .collect::<Vec<_>>(),
    )
}

fn key(value: &str) -> ModuleKey {
    ModuleKey::new(value).unwrap()
}

fn source_function(output: &CompilationOutput, module: &str, name: &str) -> SourceFunctionId {
    output
        .checked_frontend()
        .function_ids()
        .find(|function| {
            output.source_function_name(*function) == Some(name)
                && output
                    .checked_frontend()
                    .function_module(*function)
                    .and_then(|id| output.checked_frontend().module_key(id))
                    .is_some_and(|key| key.as_str() == module)
                && output.checked_frontend().function_visibility(*function)
                    != Some(FunctionVisibility::Private)
        })
        .unwrap_or_else(|| panic!("missing `{module}.{name}`"))
}
