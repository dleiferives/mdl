//! Stage 9B pinned-server proof: recurring scheduling under all four
//! optimization policies.
//!
//! Reproduces both keystone shapes named in the 9B dossier and
//! `stage-9-todo.md`'s gate line:
//!
//! - **dynamic-lights shape**: a pure self-rescheduling function, no
//!   `#minecraft:tick` tag at all;
//! - **spawn-animations shape**: a `#minecraft:tick`-registered handler
//!   *plus* a separately self-rescheduling watchdog function.
//!
//! # Why a plain `Int32` result register, polled between console-cleared windows
//!
//! Three real, load-bearing discoveries made while writing this test, all
//! about what a `tick`/schedule-target function can actually *do* and still
//! satisfy Stage 9B's checks, none anticipated by the dossier at this level
//! of detail:
//!
//! 1. An `unsafe minecraft(...)` command is conservatively modeled as
//!    requiring *every* ambient component (`ir::core::ambient`'s
//!    `UnsafeTargetFragment` handling unconditionally joins
//!    `AmbientContextRequirements::UNKNOWN`), and the *only* way to
//!    discharge the executor component is `run.as(<fresh entity query>)` —
//!    which itself unconditionally re-requires position and dimension
//!    (`transfer_run_scope_requirements`'s `AsEntityQuery` arm always joins
//!    `Required` for both, regardless of what the wrapped body needs).
//!    Consequence: a `tick`/schedule-target function containing *any*
//!    unsafe minecraft command can never be proven self-rooted today — not
//!    a rare edge case, an unconditional structural fact.
//! 2. A block-entity NBT write (BE-1/BE-2's `mc.block(Chest, x, y, z)`
//!    path) needs no ambient context, so it *is* self-rooting-compatible,
//!    but `analysis/minecraft/solve.rs` deliberately leaves
//!    `ItemReplaceBlock`'s native outcome at conservative `Unknown` (PS-16's
//!    own choice, predating Stage 9B — its exact success/fail semantics
//!    were never measured against the pinned server for an arbitrary
//!    block). `Unknown` cost can never be `ProvenWithin`, so this path is
//!    ruled out too, for a different reason than (1).
//! 3. What *is* both self-rooted and known-cost: ordinary typed `Int32`
//!    arithmetic and `return`, which lower to plain `CommandKind::Score`
//!    (scoreboard) operations — no ambient context, `Finite` cost. An
//!    exported function's `Int32` result already lives at a fixed, publicly
//!    introspectable physical scoreboard entry
//!    (`LoweredFunction::result_homes()` → `RegisterSlot::holder()`/
//!    `.objective()`), so this file reads *that* register directly from the
//!    console instead of writing anything MDL-source-observable itself.
//!    A smaller, related discovery this file also had to work around: an
//!    exported function's *callable resource* is never the source name
//!    itself (there is no `<ns>:light_tick`) — it is a generated internal
//!    path (`LoweredFunction::entry_resource()`, e.g. `<ns>:__mdl/f0/b0`),
//!    so every direct `function ...` invocation below resolves that
//!    resource through the public API rather than guessing a name.
//!
//! Each handler just returns a fixed `Int32` literal. Firing is proven by
//! resetting that same register to a sentinel from the console, stepping
//! exactly one tick, and checking the register changed — a liveness signal
//! for that specific tick, repeatable to build confidence across several
//! separately-stepped windows, and — for the period-2 `slow_cleanup`
//! watchdog — absent after the first of two ticks and present after the
//! second, directly demonstrating the configured delay rather than just
//! "it fires eventually".
//!
//! `MDL_SERVER_JAR` must point at the self-extracting bundle jar (e.g.
//! `tmp/minecraft-server-26.2.jar`), not the already-extracted
//! `versions/26.2/server-26.2.jar` directly — the extracted jar has no
//! embedded classpath and fails with `NoClassDefFoundError` when launched
//! bare (see `notes/compiler/stage-9/9-0-contracts-and-evidence.md`'s
//! References section).

use std::env;
use std::path::PathBuf;

use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{
    CompilationOptions, CompilationOutput, FrontendLimits, SourceFunctionId, SourceInput,
    compile_source,
};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::{ServerConfig, ServerSandbox, TestServer, step_and_settle};

const POLICIES: [(CoreOptimizationLevel, MinecraftOptimizationLevel); 4] = [
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

/// `dynamic-lights` shape: pure self-reschedule, no `#minecraft:tick` tag.
/// The reschedule is the function's own first statement (schedule-self-first,
/// the `dynamic-lights` robustness idiom). The fixed `Int32` return (see the
/// module doc for why) is read back from its own physical register.
const DYNAMIC_LIGHTS_SOURCE: &str = r"
export fn light_tick() -> Int32 {
    schedule light_tick, 1;
    return 1;
}
";

/// `spawn-animations` shape: a `#minecraft:tick`-registered handler plus a
/// separately self-rescheduling watchdog.
const SPAWN_ANIMATIONS_SOURCE: &str = r"
tick fn spawn_watcher() -> Int32 {
    return 1;
}

export fn slow_cleanup() -> Int32 {
    schedule slow_cleanup, 2;
    return 1;
}
";

struct Deployment {
    namespace: String,
    /// The primary (index-0 source function's) generated callable resource —
    /// exports do *not* get a resource literally named after the source
    /// function; they get a generated internal name
    /// (`LoweredFunction::entry_resource()`), a real discovery made while
    /// writing this test (see the module doc).
    entry_resource: String,
    result_holder: String,
    result_objective: String,
    watchdog_entry_resource: Option<String>,
    watchdog_holder: Option<String>,
    watchdog_objective: Option<String>,
}

fn compile_policy(
    index: usize,
    shape: &str,
    source: &str,
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
    watchdog_function_index: Option<usize>,
) -> Result<(Deployment, CompilationOutput), String> {
    let namespace = format!("mdl9b_{shape}_{index}");
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new(&namespace).unwrap(),
        ObjectiveName::new(&format!("mdl9b.{shape}{index}")).unwrap(),
    )
    .unwrap()
    .with_optimization_level(minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    let options = CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(core),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("MDL Stage 9B recurring scheduling conformance"),
    );
    let output = compile_source(SourceInput::new(format!("{shape}.mdl"), source), &options)
        .map_err(|error| format!("{shape} policy {index} compilation failed: {error}"))?;

    let function_id = |function_index: usize| -> Result<SourceFunctionId, String> {
        output
            .checked_frontend()
            .function_ids()
            .nth(function_index)
            .ok_or_else(|| format!("{shape} policy {index}: no function at index {function_index}"))
    };
    let primary_abi = output
        .source_function_abi(function_id(0)?)
        .ok_or_else(|| format!("{shape} policy {index}: first function has no lowered ABI"))?;
    let entry_resource = primary_abi.entry_resource().to_string();
    let result_home = primary_abi
        .result_homes()
        .first()
        .ok_or_else(|| format!("{shape} policy {index}: first function has no Int32 result home"))?;
    let (result_holder, result_objective) = (
        result_home.1.holder().to_string(),
        result_home.1.objective().to_string(),
    );
    let (watchdog_entry_resource, watchdog_holder, watchdog_objective) =
        match watchdog_function_index {
            Some(function_index) => {
                let abi = output
                    .source_function_abi(function_id(function_index)?)
                    .ok_or_else(|| {
                        format!("{shape} policy {index}: watchdog function has no lowered ABI")
                    })?;
                let home = abi.result_homes().first().ok_or_else(|| {
                    format!("{shape} policy {index}: watchdog function has no Int32 result home")
                })?;
                (
                    Some(abi.entry_resource().to_string()),
                    Some(home.1.holder().to_string()),
                    Some(home.1.objective().to_string()),
                )
            }
            None => (None, None, None),
        };

    Ok((
        Deployment {
            namespace,
            entry_resource,
            result_holder,
            result_objective,
            watchdog_entry_resource,
            watchdog_holder,
            watchdog_objective,
        },
        output,
    ))
}

fn command(server: &mut TestServer, text: &str) -> Result<(), String> {
    server.command(text).map_err(|error| error.to_string())
}

/// Resets a register holder to a sentinel value no real `Int32` result ever
/// equals, so a later read distinguishes "fired since the reset" from
/// "unchanged".
const SENTINEL: i32 = -1;

fn reset_register(server: &mut TestServer, holder: &str, objective: &str) -> Result<(), String> {
    command(server, &format!("scoreboard players set {holder} {objective} {SENTINEL}"))
}

/// Returns whether `holder` no longer holds the sentinel value, i.e. some
/// invocation wrote its real `Int32` result since the last reset.
fn register_fired(server: &mut TestServer, holder: &str, objective: &str) -> Result<bool, String> {
    command(server, &format!("scoreboard players get {holder} {objective}"))?;
    let line = server
        .wait_for_command_log(&format!("{holder} has"))
        .map_err(|error| error.to_string())?;
    Ok(!line.contains(&format!("{holder} has {SENTINEL} ")))
}

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn dynamic_lights_and_spawn_animations_shapes_run_under_all_four_policies() {
    if let Err(error) = run() {
        panic!("{error}");
    }
}

fn run() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR").map(PathBuf::from).ok_or_else(|| {
        "set MDL_SERVER_JAR to the official Minecraft 26.2 server bundle JAR".to_owned()
    })?;
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);

    let mut dynamic_lights = Vec::new();
    let mut spawn_animations = Vec::new();
    for (index, (core, minecraft)) in POLICIES.into_iter().enumerate() {
        dynamic_lights.push(compile_policy(
            index,
            "dynlt",
            DYNAMIC_LIGHTS_SOURCE,
            core,
            minecraft,
            None,
        )?);
        spawn_animations.push(compile_policy(
            index,
            "spwan",
            SPAWN_ANIMATIONS_SOURCE,
            core,
            minecraft,
            Some(1),
        )?);
    }

    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve).map_err(|error| error.to_string())?;
    for (deployment, output) in dynamic_lights.iter().chain(spawn_animations.iter()) {
        sandbox
            .install_datapack(
                &deployment.namespace,
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
    let result = exercise(&mut server, &dynamic_lights, &spawn_animations);
    match result {
        Ok(()) => server.shutdown().map_err(|error| error.to_string()),
        Err(error) => {
            server.preserve_sandbox();
            let _ = server.shutdown();
            Err(format!("{error}\nsandbox preserved at {}", root.display()))
        }
    }
}

fn exercise(
    server: &mut TestServer,
    dynamic_lights: &[(Deployment, CompilationOutput)],
    spawn_animations: &[(Deployment, CompilationOutput)],
) -> Result<(), String> {
    command(server, "tick freeze")?;
    server
        .wait_for_command_log("The game is frozen")
        .map_err(|error| error.to_string())?;

    for (deployment, _) in dynamic_lights.iter().chain(spawn_animations.iter()) {
        command(server, &format!("function {}:__mdl/load", deployment.namespace))?;
    }

    // dynamic-lights: arm the chain (light_tick's own first statement
    // re-schedules itself), then prove three separate 1-tick windows each
    // independently show the register changed, i.e. the chain kept firing
    // every tick, not just once.
    for (deployment, _) in dynamic_lights {
        command(server, &format!("function {}", deployment.entry_resource))?;
        for round in 0..3 {
            reset_register(server, &deployment.result_holder, &deployment.result_objective)?;
            step_and_settle(server, 1).map_err(|error| error.to_string())?;
            assert!(
                register_fired(server, &deployment.result_holder, &deployment.result_objective)?,
                "{}: light_tick did not fire during stepped tick (round {round})",
                deployment.namespace
            );
        }
    }

    // spawn-animations: the tick-tag handler needs no arming and must fire
    // every stepped tick unconditionally; the watchdog is armed once and
    // must fire on its 2-tick period specifically (unchanged after the
    // first of two ticks, changed after the second), directly demonstrating
    // the configured delay rather than just eventual firing.
    for (deployment, _) in spawn_animations {
        for round in 0..3 {
            reset_register(server, &deployment.result_holder, &deployment.result_objective)?;
            step_and_settle(server, 1).map_err(|error| error.to_string())?;
            assert!(
                register_fired(server, &deployment.result_holder, &deployment.result_objective)?,
                "{}: spawn_watcher did not fire during stepped tick (round {round})",
                deployment.namespace
            );
        }
        let watchdog_holder = deployment
            .watchdog_holder
            .as_deref()
            .expect("spawn_animations deployment records a watchdog register");
        let watchdog_objective = deployment
            .watchdog_objective
            .as_deref()
            .expect("spawn_animations deployment records a watchdog register");
        let watchdog_entry_resource = deployment
            .watchdog_entry_resource
            .as_deref()
            .expect("spawn_animations deployment records a watchdog entry resource");
        command(server, &format!("function {watchdog_entry_resource}"))?;
        reset_register(server, watchdog_holder, watchdog_objective)?;
        step_and_settle(server, 1).map_err(|error| error.to_string())?;
        assert!(
            !register_fired(server, watchdog_holder, watchdog_objective)?,
            "{}: slow_cleanup fired one tick early (period is 2)",
            deployment.namespace
        );
        step_and_settle(server, 1).map_err(|error| error.to_string())?;
        assert!(
            register_fired(server, watchdog_holder, watchdog_objective)?,
            "{}: slow_cleanup did not fire on its second tick",
            deployment.namespace
        );
    }

    // `/reload` mid-chain must not break either chain (9.0 M12/M13,
    // extended here to compiler-generated `schedule` commands specifically):
    // reset, reload, step, and confirm firing resumes exactly as before.
    command(server, "reload")?;
    server
        .wait_for_command_log("Loaded 1688 advancements")
        .map_err(|error| error.to_string())?;
    // `/reload` re-registers `#minecraft:tick`/re-parses the load tag; freeze
    // state does not necessarily survive it, so re-freeze defensively before
    // stepping again (matching 9.0's own group4 precedent).
    command(server, "tick freeze")?;
    server
        .wait_for_command_log("The game is frozen")
        .map_err(|error| error.to_string())?;

    for (deployment, _) in dynamic_lights {
        reset_register(server, &deployment.result_holder, &deployment.result_objective)?;
        step_and_settle(server, 1).map_err(|error| error.to_string())?;
        assert!(
            register_fired(server, &deployment.result_holder, &deployment.result_objective)?,
            "{}: light_tick stopped firing after /reload",
            deployment.namespace
        );
    }
    for (deployment, _) in spawn_animations {
        reset_register(server, &deployment.result_holder, &deployment.result_objective)?;
        step_and_settle(server, 1).map_err(|error| error.to_string())?;
        assert!(
            register_fired(server, &deployment.result_holder, &deployment.result_objective)?,
            "{}: spawn_watcher stopped firing after /reload",
            deployment.namespace
        );
    }

    Ok(())
}
