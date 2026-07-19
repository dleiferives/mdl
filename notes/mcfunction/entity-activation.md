# When Are Newly Summoned Entities Safe to Query?

Status: **Measured on the official Java 26.2 dedicated server on 2026-07-18.**

Successful `summon` feedback is not a sufficient synchronization condition for a
following selector-based test submitted through dedicated-server console input. In
the PS-1 batched fixture, three console `summon` commands each logged success, but a
following `execute as @e[tag=...]` still selected zero entities. Waiting for one
arbitrary scheduled tick was also too weak in the observed run.

The reliable harness protocol is condition-based:

```mcfunction
execute store result score #population mdl.test \
    run execute if entity @e[tag=mdl_test_entity]
execute if score #population mdl.test matches 3 run say MDL_ENTITIES_READY
execute unless score #population mdl.test matches 3 \
    run schedule function mdl_test:await_entities 1t replace
```

The test submits setup as independent console roots, then waits for the unique ready
marker. The scheduled probe retries once per tick until vanilla's selector observes
the declared population. Only then does it invoke the measured function.

A later PS-2 book probe exposed a stronger clientless-server prerequisite. With no
connected players, the fresh server saved/unloaded its ordinary world chunks after
startup; an armor stand could be summoned successfully and then disappear from a
following selector. Executing `forceload add 0 0` and waiting for the confirmation
before summoning made the entity stable. Clientless entity fixtures must therefore
force-load their controlled chunk before creation, not merely poll after creation.

## What this establishes

- Console command ordering proves processing order, not immediate selector
  visibility of newly created entities.
- A fixed sleep or fixed tick count is an environmental assumption, not a semantic
  barrier.
- Entity-backed tests should force-load every participating chunk and wait on a
  world-state condition.
- The polling function is harness setup in independent scheduled roots; it must not
  be included in a subject's command-sequence or fork-limit measurement.
- A bounded harness must cap the expected population and fail on timeout rather than
  polling forever.

This is a test-orchestration fact, not a claim that datapack code generally needs to
delay every command after `summon`. Commands inside one deliberately designed
function may have a stronger local ordering contract; the measured failure involved
separate roots arriving through the dedicated-server console.

## Evidence

- PS-1 batched fixture:
  [`../../crates/mdl-test/tests/ps1_calibration_suite.rs`](../../crates/mdl-test/tests/ps1_calibration_suite.rs)
- Scenario runner's scheduled readiness probe:
  [`../../crates/mdl-test/src/scenario/runner.rs`](../../crates/mdl-test/src/scenario/runner.rs)
- Earlier independent command-limit probe using the same retry shape:
  [`../../crates/mdl-test/tests/command_limits.rs`](../../crates/mdl-test/tests/command_limits.rs)
- Target server:
  <https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2>
- PS-2 written-book fixture:
  [`../../crates/mdl-test/tests/ps2_book_semantics.rs`](../../crates/mdl-test/tests/ps2_book_semantics.rs)
