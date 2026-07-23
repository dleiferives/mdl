//! Stage 9A dedicated integration tests for the `one_tick` bound contract.
//!
//! These are not fixture-shaped because the source-fixture harness
//! (`source_fixtures.rs`) always runs under the target's *default* configured
//! command limits and always expects the four optimization policies to agree.
//! Both assumptions are wrong for 9A on purpose: the contract's whole point is
//! to be sensitive to the configured limit, and it is allowed (by design, see
//! `notes/compiler/stage-9/9-a-one-tick-contract.md`) to disagree across
//! policies when optimization changes a function's actual command count.

use mdl_compiler::analysis::minecraft::{
    AnalysisArithmeticCaps, CommandLimitAssumptions, TargetExecutionAnalysisLimits,
};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{
    CompilationFailure, CompilationOptions, FrontendLimits, SourceInput, compile_source,
};
use mdl_compiler::ir::minecraft::{MinecraftDebugDumper, ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

const BOUNDED_STEP_MARKED: &str = r"
export one_tick fn bounded_step() -> Int32 {
    var result: Int32 = 0;
    result = result +% 1;
    result = result +% 1;
    result = result +% 1;
    return result;
}
";

const BOUNDED_STEP_UNMARKED: &str = r"
export fn bounded_step() -> Int32 {
    var result: Int32 = 0;
    result = result +% 1;
    result = result +% 1;
    result = result +% 1;
    return result;
}
";

fn options(
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
    command_limits: Option<CommandLimitAssumptions>,
) -> CompilationOptions {
    let mut lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("mdl_9a").unwrap(),
        ObjectiveName::new("mdl.9a").unwrap(),
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
        EmissionOptions::new("stage9a one_tick contract test"),
    )
}

/// Gate from `stage-9-todo.md`'s 9A entry: the same fixed source function must
/// prove-accept under the target default command-sequence limit and
/// prove-reject once `LoweringOptions::with_command_limit_assumptions` lowers
/// that limit below the function's actual (unoptimized) command count.
#[test]
fn two_configured_limit_settings_flip_the_same_function_from_accept_to_reject() {
    let default_options = options(
        CoreOptimizationLevel::None,
        MinecraftOptimizationLevel::None,
        None,
    );
    let passing = compile_source(
        SourceInput::new("gate.mdl", BOUNDED_STEP_MARKED),
        &default_options,
    );
    assert!(
        passing.is_ok(),
        "expected the default-limit compile to accept the one_tick contract: {}",
        passing
            .err()
            .map(|failure| failure.to_string())
            .unwrap_or_default()
    );

    let lowered_limit = CommandLimitAssumptions::new(2, 65536).unwrap();
    let strict_options = options(
        CoreOptimizationLevel::None,
        MinecraftOptimizationLevel::None,
        Some(lowered_limit),
    );
    let failing = compile_source(
        SourceInput::new("gate.mdl", BOUNDED_STEP_MARKED),
        &strict_options,
    );
    let failure = failing.expect_err("expected the lowered-limit compile to reject");
    assert!(
        matches!(failure, CompilationFailure::TargetContract { .. }),
        "expected a TargetContract failure, got: {failure}"
    );
    assert!(
        failure
            .diagnostics()
            .is_some_and(|diagnostics| diagnostics.contains_code("target-cost.one-tick-not-proven")),
        "expected target-cost.one-tick-not-proven: {failure}"
    );
}

/// The contract is evaluated per compilation, not required to agree across
/// policies (dossier, "Interaction with the four optimization policies"): Core
/// optimization can shrink an unmarked-loop-free function's redundant
/// operations enough to flip a fixed, deliberately low command-sequence limit
/// from rejecting to accepting the same source.
#[test]
fn one_tick_verdict_may_legitimately_disagree_across_optimization_policies() {
    let tight_limit = CommandLimitAssumptions::new(5, 65536).unwrap();

    let unoptimized = options(
        CoreOptimizationLevel::None,
        MinecraftOptimizationLevel::None,
        Some(tight_limit),
    );
    let unoptimized_result = compile_source(
        SourceInput::new("policy.mdl", BOUNDED_STEP_MARKED),
        &unoptimized,
    );
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
    let optimized_result = compile_source(
        SourceInput::new("policy.mdl", BOUNDED_STEP_MARKED),
        &optimized,
    );
    assert!(
        optimized_result.is_ok(),
        "expected Baseline|Baseline to accept the same source under the same limit: {}",
        optimized_result
            .err()
            .map(|failure| failure.to_string())
            .unwrap_or_default()
    );
}

/// Falsifiable claim from the dossier: a passing `one_tick`-marked function
/// emits byte-identical commands to the same function unmarked, under every
/// optimization policy. The marker participates in no lowering decision.
#[test]
fn one_tick_marker_does_not_change_emitted_commands() {
    for (core, minecraft) in [
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
    ] {
        let compile_options = options(core, minecraft, None);
        let marked = compile_source(
            SourceInput::new("marked.mdl", BOUNDED_STEP_MARKED),
            &compile_options,
        )
        .unwrap_or_else(|failure| {
            panic!("[{core:?}/{minecraft:?}] marked compile failed: {failure}")
        });
        let unmarked = compile_source(
            SourceInput::new("unmarked.mdl", BOUNDED_STEP_UNMARKED),
            &compile_options,
        )
        .unwrap_or_else(|failure| {
            panic!("[{core:?}/{minecraft:?}] unmarked compile failed: {failure}")
        });
        assert_eq!(
            MinecraftDebugDumper::program(marked.lowering().program()),
            MinecraftDebugDumper::program(unmarked.lowering().program()),
            "[{core:?}/{minecraft:?}] one_tick changed the emitted target program"
        );
        assert_eq!(
            marked.emission().pack(),
            unmarked.emission().pack(),
            "[{core:?}/{minecraft:?}] one_tick changed the emitted datapack"
        );
    }
}

/// Diagnostic completeness: two independently `one_tick`-marked, independently
/// failing functions in one program both appear in the single
/// `CompilationFailure::TargetContract`, not just the first.
#[test]
fn multiple_one_tick_violations_are_all_reported() {
    let source = r"
export one_tick fn unbounded_first(value: Int32) -> Int32 {
    var current: Int32 = value;
    var result: Int32 = 0;
    while (current > 0) {
        result = result +% current;
        current = current -% 1;
    }
    return result;
}

export one_tick fn unbounded_second(value: Int32) -> Int32 {
    var current: Int32 = value;
    var result: Int32 = 0;
    while (current > 0) {
        result = result +% current;
        current = current -% 1;
    }
    return result;
}
";
    let compile_options = options(
        CoreOptimizationLevel::None,
        MinecraftOptimizationLevel::None,
        None,
    );
    let failure = compile_source(SourceInput::new("multi.mdl", source), &compile_options)
        .expect_err("expected both unbounded one_tick functions to fail");
    let CompilationFailure::TargetContract { diagnostics, .. } = failure else {
        panic!("expected a TargetContract failure");
    };
    let violations = diagnostics
        .findings()
        .iter()
        .filter(|finding| finding.code() == "target-cost.one-tick-not-proven")
        .count();
    assert_eq!(
        violations, 2,
        "expected both marked functions to be reported, found {violations}"
    );
}
