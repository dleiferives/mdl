use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::diagnostic::render_diagnostics;
use mdl_compiler::frontend::{
    CompilationFailure, CompilationOptions, CompilationOutput, FrontendLimits, SourceInput,
    compile_source,
};
use mdl_compiler::ir::core::CanonicalPrinter;
use mdl_compiler::ir::minecraft::{MinecraftDebugDumper, ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

const FIXTURE_FILTER: &str = "MDL_FIXTURE_FILTER";

#[derive(Clone, Copy, Debug)]
struct Policy {
    label: &'static str,
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
}

const POLICIES: [Policy; 4] = [
    Policy {
        label: "core-none_minecraft-none",
        core: CoreOptimizationLevel::None,
        minecraft: MinecraftOptimizationLevel::None,
    },
    Policy {
        label: "core-none_minecraft-baseline",
        core: CoreOptimizationLevel::None,
        minecraft: MinecraftOptimizationLevel::Baseline,
    },
    Policy {
        label: "core-baseline_minecraft-none",
        core: CoreOptimizationLevel::Baseline,
        minecraft: MinecraftOptimizationLevel::None,
    },
    Policy {
        label: "core-baseline_minecraft-baseline",
        core: CoreOptimizationLevel::Baseline,
        minecraft: MinecraftOptimizationLevel::Baseline,
    },
];

#[derive(Clone, Debug, Eq, PartialEq)]
enum ExpectedResult {
    Success,
    Failure { phase: Box<str>, code: Box<str> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArtifactKind {
    Hir,
    Core,
    Lowering,
    Target,
    Pack,
}

impl ArtifactKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Hir => "hir",
            Self::Core => "core",
            Self::Lowering => "lowering",
            Self::Target => "target",
            Self::Pack => "pack",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Hir => 0,
            Self::Core => 1,
            Self::Lowering => 2,
            Self::Target => 3,
            Self::Pack => 4,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Check {
    artifact: ArtifactKind,
    expected_count: Option<usize>,
    ordered: bool,
    pattern: Box<str>,
    source_line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Fixture {
    path: PathBuf,
    name: Box<str>,
    source: Box<str>,
    expected: ExpectedResult,
    checks: Box<[Check]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Artifacts {
    hir: String,
    core: String,
    lowering: String,
    target: String,
    pack: String,
}

impl Artifacts {
    fn get(&self, artifact: ArtifactKind) -> &str {
        match artifact {
            ArtifactKind::Hir => &self.hir,
            ArtifactKind::Core => &self.core,
            ArtifactKind::Lowering => &self.lowering,
            ArtifactKind::Target => &self.target,
            ArtifactKind::Pack => &self.pack,
        }
    }
}

#[test]
fn source_fixtures_cross_every_requested_compiler_boundary() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/source-fixtures");
    let filter = env::var(FIXTURE_FILTER).ok();
    let fixtures = discover_fixtures(&root, filter.as_deref());
    assert!(
        !fixtures.is_empty(),
        "no source fixtures matched {}",
        filter.as_deref().unwrap_or("<all>")
    );
    for fixture in fixtures {
        run_fixture(&fixture);
    }
}

fn discover_fixtures(root: &Path, filter: Option<&str>) -> Vec<Fixture> {
    let mut paths = Vec::new();
    collect_fixture_paths(root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .map(|path| parse_fixture(&path))
        .filter(|fixture| filter.is_none_or(|filter| fixture.name.contains(filter)))
        .collect()
}

fn collect_fixture_paths(directory: &Path, paths: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory).unwrap_or_else(|error| {
        panic!(
            "cannot read fixture directory {}: {error}",
            directory.display()
        )
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "cannot read an entry below {}: {error}",
                directory.display()
            )
        });
        let path = entry.path();
        if path.is_dir() {
            collect_fixture_paths(&path, paths);
        } else if path.extension().is_some_and(|extension| extension == "mdl") {
            paths.push(path);
        }
    }
}

fn parse_fixture(path: &Path) -> Fixture {
    let source = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read fixture {}: {error}", path.display()));
    let mut expected = None;
    let mut checks = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let source_line = index + 1;
        let Some(directive) = line.trim_start().strip_prefix("// ") else {
            continue;
        };
        if let Some(value) = directive.strip_prefix("MDL: ") {
            assert!(
                expected.is_none(),
                "{}:{source_line}: duplicate MDL result",
                path.display()
            );
            expected = Some(parse_expected_result(path, source_line, value));
            continue;
        }
        if let Some((key, pattern)) = directive.split_once(": ") {
            if let Some((artifact, expected_count, ordered)) = parse_check_key(key) {
                assert!(
                    !pattern.is_empty(),
                    "{}:{source_line}: empty {key} pattern",
                    path.display()
                );
                checks.push(Check {
                    artifact,
                    expected_count,
                    ordered,
                    pattern: pattern.into(),
                    source_line,
                });
            } else if starts_like_harness_directive(key) {
                panic!(
                    "{}:{source_line}: malformed fixture directive `{key}`",
                    path.display()
                );
            }
        } else if starts_like_harness_directive(directive) {
            panic!(
                "{}:{source_line}: malformed fixture directive `{directive}`",
                path.display()
            );
        }
    }
    let expected = expected.unwrap_or_else(|| {
        panic!(
            "{}: fixture needs `// MDL: success` or `// MDL: failure <phase> <code>`",
            path.display()
        )
    });
    if matches!(expected, ExpectedResult::Failure { .. }) {
        assert!(
            checks.is_empty(),
            "{}: failure fixtures cannot inspect unavailable success artifacts",
            path.display()
        );
    } else {
        assert!(
            !checks.is_empty(),
            "{}: success fixture has no semantic checks",
            path.display()
        );
    }
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| panic!("fixture path is not UTF-8: {}", path.display()));
    Fixture {
        path: path.to_path_buf(),
        name: name.into(),
        source: source.into(),
        expected,
        checks: checks.into_boxed_slice(),
    }
}

fn starts_like_harness_directive(value: &str) -> bool {
    ["MDL", "HIR", "CORE", "LOWERING", "TARGET", "PACK"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

fn parse_expected_result(path: &Path, line: usize, value: &str) -> ExpectedResult {
    if value == "success" {
        return ExpectedResult::Success;
    }
    let Some(rest) = value.strip_prefix("failure ") else {
        panic!("{}:{line}: invalid MDL result `{value}`", path.display());
    };
    let Some((phase, code)) = rest.split_once(' ') else {
        panic!(
            "{}:{line}: failure needs `<phase> <diagnostic-code>`",
            path.display()
        );
    };
    assert!(
        !phase.is_empty() && !code.is_empty(),
        "{}:{line}: failure phase and code must be nonempty",
        path.display()
    );
    ExpectedResult::Failure {
        phase: phase.into(),
        code: code.into(),
    }
}

fn parse_check_key(key: &str) -> Option<(ArtifactKind, Option<usize>, bool)> {
    let (artifact, suffix) = key.split_once('-').unwrap_or((key, ""));
    let artifact = match artifact {
        "HIR" => ArtifactKind::Hir,
        "CORE" => ArtifactKind::Core,
        "LOWERING" => ArtifactKind::Lowering,
        "TARGET" => ArtifactKind::Target,
        "PACK" => ArtifactKind::Pack,
        _ => return None,
    };
    let (expected_count, ordered) = match suffix {
        "" => (None, false),
        "NOT" => (Some(0), false),
        "ORDER" => (None, true),
        suffix => {
            let count = suffix.strip_prefix("COUNT-")?.parse::<usize>().ok()?;
            (Some(count), false)
        }
    };
    Some((artifact, expected_count, ordered))
}

fn run_fixture(fixture: &Fixture) {
    for policy in POLICIES {
        let options = options(policy);
        let first = compile_source(
            SourceInput::new(fixture.path.to_string_lossy(), fixture.source.clone()),
            &options,
        );
        match &fixture.expected {
            ExpectedResult::Success => {
                let first = first.unwrap_or_else(|failure| {
                    panic!(
                        "{} [{}] unexpectedly failed at {}:\n{}",
                        fixture.name,
                        policy.label,
                        failure_phase(&failure),
                        render_failure(&failure)
                    )
                });
                let second = compile_source(
                    SourceInput::new(fixture.path.to_string_lossy(), fixture.source.clone()),
                    &options,
                )
                .unwrap_or_else(|failure| {
                    panic!(
                        "{} [{}] repeated compilation failed: {failure}",
                        fixture.name, policy.label
                    )
                });
                let first_artifacts = artifacts(&first);
                let second_artifacts = artifacts(&second);
                assert_eq!(
                    first_artifacts, second_artifacts,
                    "{} [{}] is not deterministic",
                    fixture.name, policy.label
                );
                assert_eq!(
                    first.emission().pack(),
                    second.emission().pack(),
                    "{} [{}] emitted pack is not deterministic",
                    fixture.name,
                    policy.label
                );
                check_artifacts(fixture, policy, &first_artifacts);
            }
            ExpectedResult::Failure { phase, code } => {
                let Err(failure) = first else {
                    panic!(
                        "{} [{}] unexpectedly compiled successfully",
                        fixture.name, policy.label
                    );
                };
                let Err(repeated) = compile_source(
                    SourceInput::new(fixture.path.to_string_lossy(), fixture.source.clone()),
                    &options,
                ) else {
                    panic!(
                        "{} [{}] repeated compilation unexpectedly succeeded",
                        fixture.name, policy.label
                    );
                };
                assert_eq!(
                    failure_phase(&failure),
                    phase.as_ref(),
                    "{} [{}] failed at the wrong phase:\n{}",
                    fixture.name,
                    policy.label,
                    render_failure(&failure)
                );
                let diagnostics = failure_diagnostics(&failure);
                assert!(
                    diagnostics.is_some_and(|diagnostics| diagnostics.contains_code(code)),
                    "{} [{}] did not report {}:\n{}",
                    fixture.name,
                    policy.label,
                    code,
                    render_failure(&failure)
                );
                assert_eq!(
                    failure_phase(&failure),
                    failure_phase(&repeated),
                    "{} [{}] failure phase is not deterministic",
                    fixture.name,
                    policy.label
                );
                assert_eq!(
                    render_failure(&failure),
                    render_failure(&repeated),
                    "{} [{}] failure diagnostics are not deterministic",
                    fixture.name,
                    policy.label
                );
            }
        }
    }
}

fn options(policy: Policy) -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("mdl_fixture").unwrap(),
        ObjectiveName::new("mdl.fixture").unwrap(),
    )
    .unwrap()
    .with_optimization_level(policy.minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(policy.core),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("MDL source fixture"),
    )
}

fn artifacts(output: &CompilationOutput) -> Artifacts {
    Artifacts {
        hir: output.checked_frontend().dump(output.sources()),
        core: CanonicalPrinter::new(output.core_optimization().program(), output.sources())
            .unwrap()
            .render(),
        lowering: output.lowering().dump_lowering(),
        target: MinecraftDebugDumper::program(output.lowering().program()),
        pack: pack_dump(output),
    }
}

fn pack_dump(output: &CompilationOutput) -> String {
    let mut dump = String::new();
    for file in output.emission().pack().files() {
        let _ = writeln!(dump, "== {} ==", file.path().as_str());
        if let Ok(text) = std::str::from_utf8(file.bytes()) {
            dump.push_str(text);
        } else {
            for byte in file.bytes() {
                let _ = write!(dump, "{byte:02x}");
            }
            dump.push('\n');
        }
        if !dump.ends_with('\n') {
            dump.push('\n');
        }
    }
    dump
}

fn check_artifacts(fixture: &Fixture, policy: Policy, artifacts: &Artifacts) {
    let mut ordered_offsets = [0; 5];
    for check in &fixture.checks {
        let actual = artifacts.get(check.artifact);
        if check.ordered {
            let offset = &mut ordered_offsets[check.artifact.index()];
            let Some(position) = actual[*offset..].find(check.pattern.as_ref()) else {
                let failure_dir = write_failure_artifacts(fixture, policy, artifacts);
                panic!(
                    "{}:{} [{}] expected {} to contain {:?} after byte offset {}; actual artifacts: {}",
                    fixture.path.display(),
                    check.source_line,
                    policy.label,
                    check.artifact.name(),
                    check.pattern,
                    *offset,
                    failure_dir.display()
                );
            };
            *offset += position + check.pattern.len();
            continue;
        }
        let count = actual.matches(check.pattern.as_ref()).count();
        let passed = check
            .expected_count
            .map_or(count != 0, |expected| count == expected);
        if !passed {
            let failure_dir = write_failure_artifacts(fixture, policy, artifacts);
            let expectation = check.expected_count.map_or_else(
                || "at least once".to_owned(),
                |count| format!("exactly {count} time(s)"),
            );
            panic!(
                "{}:{} [{}] expected {} to contain {:?} {expectation}, found {count}; actual artifacts: {}",
                fixture.path.display(),
                check.source_line,
                policy.label,
                check.artifact.name(),
                check.pattern,
                failure_dir.display()
            );
        }
    }
}

fn write_failure_artifacts(fixture: &Fixture, policy: Policy, artifacts: &Artifacts) -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/mdl-fixture-failures")
        .join(fixture.name.as_ref())
        .join(policy.label);
    fs::create_dir_all(&directory).unwrap_or_else(|error| {
        panic!(
            "cannot create fixture failure directory {}: {error}",
            directory.display()
        )
    });
    for artifact in [
        ArtifactKind::Hir,
        ArtifactKind::Core,
        ArtifactKind::Lowering,
        ArtifactKind::Target,
        ArtifactKind::Pack,
    ] {
        let path = directory.join(format!("{}.txt", artifact.name()));
        fs::write(&path, artifacts.get(artifact))
            .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
    }
    directory
}

fn failure_phase(failure: &CompilationFailure) -> &'static str {
    match failure {
        CompilationFailure::PackageInput(_) => "package-input",
        CompilationFailure::SourceInput(_) => "source-input",
        CompilationFailure::FrontendInfrastructure { .. } => "frontend-infrastructure",
        CompilationFailure::Syntax { .. } => "syntax",
        CompilationFailure::Semantic { .. } => "semantic",
        CompilationFailure::CoreGeneration { .. } => "core-generation",
        CompilationFailure::CoreOptimization { .. } => "core-optimization",
        CompilationFailure::MinecraftLowering { .. } => "minecraft-lowering",
        CompilationFailure::DatapackEmission { .. } => "datapack-emission",
        _ => "unknown",
    }
}

fn failure_diagnostics(
    failure: &CompilationFailure,
) -> Option<&mdl_compiler::diagnostic::Diagnostics> {
    failure.diagnostics().or_else(|| {
        failure
            .minecraft_lowering_failure()
            .map(mdl_compiler::lower::minecraft::LoweringFailure::diagnostics)
    })
}

fn render_failure(failure: &CompilationFailure) -> String {
    let Some(diagnostics) = failure_diagnostics(failure) else {
        let mut rendered = failure.to_string();
        if let (Some(checked), Some(sources)) = (failure.checked_frontend(), failure.sources()) {
            rendered.push_str("\n\nretained checked HIR:\n");
            rendered.push_str(&checked.dump(sources));
        }
        return rendered;
    };
    failure.sources().map_or_else(
        || diagnostics.to_string(),
        |sources| render_diagnostics(diagnostics, sources),
    )
}
