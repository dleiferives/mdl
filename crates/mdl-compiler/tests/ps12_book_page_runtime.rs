use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{CompilationOptions, FrontendLimits, SourceInput, compile_source};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

// Deliberately not under `tests/source-fixtures/`: that tree is scanned by
// `source_fixtures.rs`'s generic four-policy differential harness (PS-12D's
// job), which would constant-fold `idx` under `CoreOptimizationLevel::
// Baseline` and never exercise the runtime/macro path this test targets.
const SOURCE: &str = r#"
export fn read_book() {
    run.as(mc.entities(ArmorStand).with_tag("mdl_ps12_book").limit(1)) |reader| {
        const idx: Int32 = 0;
        const page: String = reader.main_hand_written_book_literal_page_or_empty(idx);
        if (page.ends_with_ascii("+")) {
            reader.say("MDL_PS12_BOOK_OK");
        }
    }
}
"#;

/// PS-12.0 regression: a genuinely runtime (non-literal) book-page index must
/// route its read to the caller's real result home, not the `mdl:preflight`
/// scratch placeholder that nothing else ever reads. This fails against the
/// pre-fix tree: the macro helper writes to `mdl:preflight "book_page"`
/// while the caller's `ends_with_ascii` check reads the value's real,
/// independently assigned home storage — two different addresses.
#[test]
fn runtime_book_page_index_routes_to_caller_result_home() {
    let output = compile_source(SourceInput::new("ps12.mdl", SOURCE), &options()).unwrap();
    let pack = output
        .emission()
        .pack()
        .files()
        .iter()
        .filter_map(|file| std::str::from_utf8(file.bytes()).ok())
        .collect::<Vec<_>>()
        .join("\n");

    // Locate the macro-rendered entity read line, e.g.:
    //   $data modify storage <target> set from entity @s .equipment...
    let read_line = pack
        .lines()
        .find(|line| line.contains("set from entity") && line.contains("data modify storage"))
        .expect("macro-rendered book-page read line must be present");
    let write_target = read_line
        .split("data modify storage ")
        .nth(1)
        .and_then(|rest| rest.split(" set from entity").next())
        .expect("read line has a `data modify storage <target>` prefix")
        .trim();

    assert!(
        !write_target.starts_with("mdl:preflight"),
        "runtime book-page read still targets the unconsumed scratch placeholder: {write_target}\nfull pack:\n{pack}"
    );

    // The value's real home is independently referenced by the caller's
    // `ends_with_ascii` lowering (`set string storage <home> ... -1`). The
    // fix requires the macro write target to be that exact same address.
    let consuming_line = format!("set string storage {write_target}");
    assert!(
        pack.contains(&consuming_line),
        "macro write target {write_target} is never read back by the caller\nfull pack:\n{pack}"
    );
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps12_book").unwrap(),
        ObjectiveName::new("ps12.book").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::None);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::None),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-12.0 regression"),
    )
}
