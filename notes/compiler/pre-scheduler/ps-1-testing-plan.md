# PS-1: Semantic and Vanilla Testing Foundation

Status: **complete (2026-07-18; see [`ps-1-handoff.md`](ps-1-handoff.md))**

## Objective

Make semantic regressions cheap to express and failures cheap to understand before
the language surface expands. PS-1 builds a target-independent evaluator for the
supported Core subset and a state-based scenario runner around the real vanilla
server. It does not emulate Minecraft.

## Inherited evidence

The repository already has:

- typed Rust unit, verifier, corruption, and scale tests;
- source fixtures with `HIR`, `CORE`, `LOWERING`, `TARGET`, and `PACK` checks;
- `None|Baseline` Core and Minecraft policy products;
- repeated compilation and byte-determinism checks;
- explicit exact-golden blessing for the Stage 4 compatibility oracle;
- a disposable clientless vanilla-server harness;
- pinned server hashes and generated command-report checks; and
- handwritten Stage 7.5/8 runtime and command-limit probes.

PS-1 extends these layers rather than replacing them.

## Test contracts

Use the narrowest authority that proves the requested fact:

| Question | Authority |
| --- | --- |
| Does a pure program compute the right value? | Core evaluator |
| Did a pass retain or transform the intended structure? | Typed query or phase pattern |
| Did the backend select a particular recipe or representation? | Lowering/target report |
| Is complete serialization deliberately stable? | Exact reviewed golden |
| Does Minecraft parse and execute the generated behavior? | Pinned vanilla server |
| Is a hard command-limit bound accurate? | Static report plus bare boundary probe |
| Is an implementation faster? | Separate repeatable measurement protocol |

Generated pack text is not a substitute for execution semantics. Conversely, a
server result cannot prove that the compiler selected a required cost or safety
strategy.

## Core evaluator boundary

Start as a module inside the compiler or test support rather than creating a crate
before a dependency boundary exists. It consumes verified Core and an exported
function plus typed arguments. It produces typed returned values or a structured
evaluation failure.

The first supported subset is the Core vocabulary already executable through the
backend:

- scalar constants;
- pure scalar instructions;
- block parameters, jumps, branches, returns, and unreachable;
- ordinary functions and multiple results;
- direct/mutual recursion under an explicit evaluator step/depth budget; and
- deterministic external-operation rejection.

The evaluator must not call the lowering pipeline, use physical homes, or duplicate
Minecraft behavior. Later PS-2 operations extend it only after their Core semantics
are frozen.

Every evaluation has explicit resource limits. Limit exhaustion is a test-harness
outcome, not an MDL value and not evidence that the evaluated program semantically
diverges.

## Minecraft scenario model

A scenario is a controlled state-transition contract:

```text
MinecraftScenario {
  target identity
  compiler policies
  source/package input
  initial world projection
  invocation frame
  gamerule assumptions
  entry operation
  expected world projection
  expected success/result/completion
  optional cost/limit contract
}
```

The typed Rust API should own orchestration. Data files may hold source and case
values, but PS-1 should not create an arbitrary shell-command language in comments.
Unusual target research can continue to use focused Rust tests.

### Isolation

- Use a fresh disposable world per server suite, not necessarily per case.
- Give each scenario a stable namespace, storage root, entity tag, and generation
  nonce.
- Force-load every chunk whose entities/blocks participate.
- Reset all declared observable state before each policy execution.
- Clean test entities, blocks, storage, objectives, bossbars, schedules, and
  gamerules owned by the scenario.
- Preserve the sandbox and complete artifacts on failure.

### Invocation frame

Make executor, position, rotation, dimension, and anchor explicit when a scenario
depends on them. Do not infer an entity-relative frame from a receiver. Cardinality
and child identity are observations, not merely setup details.

### Observation

Compare only a declared projection of world state:

- exported scalar outcomes;
- stable test-owned scoreboard values;
- test-owned storage NBT;
- selected blocks/block entities;
- normalized entity records; and
- completion/continuation markers.

Do not snapshot a complete world directory. Ignore UUIDs, timestamps, generated
resource names, and selector iteration order unless the language contract exposes
them. Possibly-many results default to set/multiset comparison.

Logs are a transport and debugging artifact. They are the semantic observation only
for behavior such as `say` attribution whose public effect is the server message.

## Outcome and completion protocol

Minecraft command `success` and `result` remain separate. A scenario that tests
outcomes should use a generated harness wrapper to store both channels and an
independent continuation marker. Zero contexts, command failure, successful result
zero, `return 0`, and abnormal command-sequence termination must remain
distinguishable.

The implemented wrapper stores `success` and `result` into two independently absent
or present NBT paths. It does not use an integer sentinel: every `i32`, including the
minimum value, remains available as an ordinary result. Before invocation the runner
removes the policy's outcome compound and sets its continuation score to zero. The
first wrapper line after the observed command sets continuation to one; ABI result
publication follows and cannot redefine whether the observed command returned.

After submitting the wrapper root, the runner immediately submits a unique
generation-and-policy console `say` barrier. Dedicated-server stdin is ordered, so
observing that barrier proves the preceding root has synchronously returned or
aborted without a timing sleep. The runner then queries path presence and values from
the new console root. Outcome-channel availability, channel value, and outer
continuation are three independent facts.

Synchronous cases may be queried after the invocation returns. Future scheduled
cases use a generation nonce and explicit `done` state rather than an unscoped log
substring.

Mojang's `return run` contract independently confirms why result availability is not
just a numeric value: when the nested command produces no value, `return` does not
execute and the function continues; a failed nested command instead supplies return
value zero. See [Java Edition 1.20.3](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-3).

## Policy differential

Compile each suitable source case under all four policies and install variants with
distinct namespaces/objectives into one sandbox. Reset state and invoke them
sequentially. Compare their normalized observations with each other and with an
explicit expectation or Core-evaluator result.

This is semantic equality, not pack equality. Compiler-private scores, frame paths,
function names, and command layout may differ.

## Instrumentation and exact cost

Observation commands change sequence cost. Keep separate modes:

- semantic scenarios use high hard limits and an observation wrapper;
- exact hard-limit probes invoke the measured root without injected tracing and
  inspect state afterward from a new console root; and
- wall-time/macro-cache measurements use the existing measurement protocol with
  warmup, repetitions, and complete environment metadata.

Static cost reports remain compiler facts. Selected vanilla boundary probes validate
their target assumptions; server timing does not replace them.

The implemented exact-limit runner has no generated driver pack. It submits adapter
prelude commands as independent console roots, invokes the deployed entry directly,
waits on a later console barrier, then runs publication and typed queries as new
roots. An exact-limit contract names a test-owned completion score. The measured
function itself must set that score to one as its final command, so the completion
write is explicit in—and counted as part of—the contract being calibrated. The
runner initializes it to zero outside the measured root and never inserts hidden
commands into the measured sequence. Validation requires that score to be test-owned
and keeps it out of the ordinary initial/expected projection so completion has one
authority rather than two potentially contradictory expectations.

The runner applies the configured gamerule immediately before each policy root. Its
external barrier is a single direct command and therefore remains executable at the
effective minimum sequence quota. The runner restores `65,536` immediately after
that barrier, before ABI publication or multi-stage observation queries; otherwise a
limit-zero/one probe could accidentally limit the harness query rather than only the
subject.

## Initial calibration corpus

PS-1 proves itself using already-understood behavior:

1. scalar constant/call/branch results;
2. direct and mutual recursion with normal cleanup;
3. `execute as` over zero, one, and many controlled entities;
4. incoming-frame versus executor-relative movement;
5. nested serial run children without state contamination;
6. normal return, failure, result zero, and no-context contrasts;
7. exact sequence/fork limit boundaries; and
8. sequence-abort residue followed by explicit recovery.

Do not delete the existing handwritten evidence until the new cases demonstrate an
equal or stronger contract and clearer failures.

## Non-goals

- a general mcfunction interpreter;
- a complete world snapshot/diff system;
- a declarative language for arbitrary console commands;
- automatic blessing in CI;
- timing thresholds in the ordinary test suite;
- connected-client automation; and
- PS-2 aggregate/string semantics.

## Exit criteria

- Supported pure Core programs execute deterministically with bounded evaluator
  resources.
- Existing scalar programs agree between Core evaluation and vanilla execution.
- All four compiler policies can execute in one server lifecycle and produce equal
  normalized observations.
- State mismatches identify the case, policy, path, expected value, and actual value.
- No test needs to parse compiler-private runtime storage to assert source behavior.
- Semantic and exact-limit modes cannot be accidentally mixed.
- Calibration cases cover context, outcome, recursion, cleanup, and both hard limits.
- The workflow and failure-artifact locations are documented.
