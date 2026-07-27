//! Stage 9B dedicated integration tests for recurring scheduling.
//!
//! Mirrors `stage9a_one_tick_contract.rs`'s shape: these are not
//! fixture-shaped because the source-fixture harness always runs under the
//! target's default configured command limits and expects the four
//! optimization policies to agree, both of which this tranche deliberately
//! violates on purpose (a lowered configured limit, and policy-sensitive
//! folding), exactly as 9A's own dedicated tests already established the
//! pattern for.

use mdl_compiler::analysis::minecraft::{
    AnalysisArithmeticCaps, CommandLimitAssumptions, TargetExecutionAnalysisLimits,
};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{
    CompilationFailure, CompilationOptions, FrontendLimits, SourceInput, compile_source,
};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

fn options(
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
    command_limits: Option<CommandLimitAssumptions>,
) -> CompilationOptions {
    let mut lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("mdl_9b").unwrap(),
        ObjectiveName::new("mdl.9b").unwrap(),
    )
    .unwrap()
    .with_optimization_level(minecraft);
    if let Some(command_limits) = command_limits {
        lowering = lowering.with_command_limit_assumptions(command_limits);
    }
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(core),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("stage9b recurring scheduling test"),
    )
}

const TWO_CHEAP_TICK_HANDLERS: &str = r"
tick fn handler_a() {
    var result: Int32 = 0;
    result = result +% 1;
    result = result +% 1;
    result = result +% 1;
}

tick fn handler_b() {
    var result: Int32 = 0;
    result = result +% 1;
    result = result +% 1;
    result = result +% 1;
}
";

/// Two individually cheap tick handlers whose *combined* sequence cost
/// exceeds a deliberately lowered configured limit must be rejected
/// together via the tag's one aggregate root — proving the tag's aggregate
/// cost, not a per-handler sum the compiler would have to compute itself
/// some other way, is what gets checked (dossier: "Aggregate tick-tag
/// budget").
#[test]
fn aggregate_tick_tag_budget_rejects_combined_over_limit_handlers() {
    let default_options = options(
        CoreOptimizationLevel::None,
        MinecraftOptimizationLevel::None,
        None,
    );
    let passing = compile_source(
        SourceInput::new("default.mdl", TWO_CHEAP_TICK_HANDLERS),
        &default_options,
    );
    assert!(
        passing.is_ok(),
        "expected the default-limit compile to accept both tick handlers: {}",
        passing
            .err()
            .map(|failure| failure.to_string())
            .unwrap_or_default()
    );

    // Each handler alone is a handful of sequence operations; a limit large
    // enough for one handler but not both combined must reject.
    let lowered_limit = CommandLimitAssumptions::new(8, 65536).unwrap();
    let strict_options = options(
        CoreOptimizationLevel::None,
        MinecraftOptimizationLevel::None,
        Some(lowered_limit),
    );
    let failing = compile_source(
        SourceInput::new("strict.mdl", TWO_CHEAP_TICK_HANDLERS),
        &strict_options,
    );
    let failure = failing.expect_err("expected the combined tick-tag cost to reject");
    assert!(
        matches!(failure, CompilationFailure::TargetContract { .. }),
        "expected a TargetContract failure, got: {failure}"
    );
    assert!(
        failure
            .diagnostics()
            .is_some_and(|diagnostics| diagnostics.contains_code("target-cost.tick-tag-not-proven")),
        "expected target-cost.tick-tag-not-proven: {failure}"
    );
}

/// Self-reschedule allocates no Stage 8 activation frame: a self-scheduling
/// function contributes zero recursive call occurrences, while an ordinary
/// self-recursive function in the same program still correctly contributes
/// at least one — the positive control proving the zero above is meaningful,
/// not an accident of an empty program.
#[test]
fn self_reschedule_is_not_recursion() {
    let self_reschedule_only = r"
fn light_tick() {
    schedule light_tick, 5;
}
";
    let compile_options = options(
        CoreOptimizationLevel::None,
        MinecraftOptimizationLevel::None,
        None,
    );
    let output = compile_source(
        SourceInput::new("self_reschedule.mdl", self_reschedule_only),
        &compile_options,
    )
    .unwrap_or_else(|failure| panic!("self-reschedule-only compile failed: {failure}"));
    assert_eq!(
        output
            .lowering()
            .report()
            .statistics()
            .recursive_call_occurrences(),
        0,
        "a self-scheduling function must allocate no Stage 8 activation frame"
    );

    let ordinary_recursion = r"
fn countdown(n: Int32) -> Int32 {
    if (n <= 0) {
        return 0;
    }
    return countdown(n -% 1);
}

export fn run_countdown() -> Int32 {
    return countdown(3);
}
";
    let output = compile_source(
        SourceInput::new("ordinary_recursion.mdl", ordinary_recursion),
        &compile_options,
    )
    .unwrap_or_else(|failure| panic!("ordinary-recursion compile failed: {failure}"));
    assert!(
        output
            .lowering()
            .report()
            .statistics()
            .recursive_call_occurrences()
            > 0,
        "an ordinary self-recursive function must still be classified recursive"
    );
}

/// Reuses 9A's own discovered mechanism (straight-line redundant-operation
/// folding differs by optimization policy, not loop elimination — no loop is
/// ever provably finite under any policy): a tick handler that is
/// `ProvenWithin` under `Baseline|Baseline` but not under `None|None`, at a
/// fixed configured limit, for the aggregate tag root.
#[test]
fn tick_tag_verdict_may_legitimately_disagree_across_optimization_policies() {
    let source = r"
tick fn redundant_tick() {
    var result: Int32 = 0;
    result = result +% 1;
    result = result +% 1;
    result = result +% 1;
}
";
    let tight_limit = CommandLimitAssumptions::new(5, 65536).unwrap();

    let unoptimized = options(
        CoreOptimizationLevel::None,
        MinecraftOptimizationLevel::None,
        Some(tight_limit),
    );
    let unoptimized_result =
        compile_source(SourceInput::new("policy.mdl", source), &unoptimized);
    assert!(
        matches!(
            unoptimized_result,
            Err(CompilationFailure::TargetContract { .. })
        ),
        "expected None|None to reject under the tight limit"
    );

    let optimized = options(
        CoreOptimizationLevel::Baseline,
        MinecraftOptimizationLevel::Baseline,
        Some(tight_limit),
    );
    let optimized_result = compile_source(SourceInput::new("policy.mdl", source), &optimized);
    assert!(
        optimized_result.is_ok(),
        "expected Baseline|Baseline to accept the same source under the same limit: {}",
        optimized_result
            .err()
            .map(|failure| failure.to_string())
            .unwrap_or_default()
    );
}

/// Diagnostic completeness: two independently failing schedule-target
/// functions in one program both appear in the single
/// `CompilationFailure::TargetContract`'s diagnostics, not just the first.
#[test]
fn multiple_schedule_target_violations_are_all_reported() {
    let source = r"
fn unbounded_first() -> Int32 {
    var current: Int32 = 5;
    var result: Int32 = 0;
    while (current > 0) {
        result = result +% current;
        current = current -% 1;
    }
    return result;
}

fn unbounded_second() -> Int32 {
    var current: Int32 = 5;
    var result: Int32 = 0;
    while (current > 0) {
        result = result +% current;
        current = current -% 1;
    }
    return result;
}

fn arm_both() {
    schedule unbounded_first, 5;
    schedule unbounded_second, 5;
}
";
    let compile_options = options(
        CoreOptimizationLevel::None,
        MinecraftOptimizationLevel::None,
        None,
    );
    let failure = compile_source(SourceInput::new("multi.mdl", source), &compile_options)
        .expect_err("expected both unbounded schedule targets to fail");
    let CompilationFailure::TargetContract { diagnostics, .. } = failure else {
        panic!("expected a TargetContract failure");
    };
    let violations = diagnostics
        .findings()
        .iter()
        .filter(|finding| finding.code() == "target-cost.schedule-target-not-proven")
        .count();
    assert_eq!(
        violations, 2,
        "expected both marked functions to be reported, found {violations}"
    );
}
