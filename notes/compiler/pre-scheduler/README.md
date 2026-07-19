# Stage 8.5: Pre-Scheduler Capability Validation

Status: **PS-1 through PS-3 complete; PS-4 enums/switch/ranges and PS-5 anonymous
structs/destructuring planned**

Stage 8.5 sits between the completed scalar synchronous Stage 8 and Stage 9's
persistent, multi-tick scheduler. Its purpose is to make MDL capable of expressing,
compiling, and validating substantial synchronous programs before scheduling adds a
second activation lifetime.

The first capstone is a Brainfuck interpreter whose program is ultimately obtained
from a written book. Brainfuck is a demanding client, not a source of
compiler-specific shortcuts. Every capability added for it must have coherent
language semantics, verified IR, independently testable lowering, and a reusable
public surface.

## Accepted substages

| Substage | Outcome | Plan | Checklist |
| --- | --- | --- | --- |
| PS-1 | Semantic and vanilla-server test oracles | [plan](ps-1-testing-plan.md) | [complete handoff](ps-1-handoff.md) |
| PS-2 | General synchronous capabilities required by Brainfuck | [plan](ps-2-capability-expansion-plan.md) | [complete handoff](ps-2-handoff.md) |
| PS-3 | Complete Brainfuck capstone compiled and run on vanilla | [plan](ps-3-brainfuck-capstone-plan.md) | [complete handoff](ps-3-handoff.md) |
| PS-4 | Closed enums, exhaustive switch, and inclusive range patterns | [plan](ps-4-enums-switch-ranges-plan.md) | [checklist](ps-4-enums-switch-ranges-todo.md) |
| PS-5 | Anonymous structs, multiple results, and destructuring | [plan](ps-5-anon-structs-destructuring-plan.md) | [checklist](ps-5-anon-structs-destructuring-todo.md) |

The exact implemented PS-2 boundary and remaining nonblocking breadth are frozen in
the [PS-2 to PS-3 handoff](ps-2-handoff.md).
The completed capstone evidence and persistent-state requirements are in the
[PS-3 to Stage 9 handoff](ps-3-handoff.md).

PS-1 through PS-3 are the frozen original required path. PS-4 and PS-5 are
explicitly accepted post-capstone language-capability milestones, ordered PS-4 then
PS-5 by work order, but Stage 9 does not semantically depend on enums or multiple
results. Later validation programs are accepted individually using
[the validation-program template](validation-program-template.md).

PS-2.0's frozen language, budget, book, and ownership choices are recorded in
[`ps-2-0-decisions.md`](ps-2-0-decisions.md).
They do not silently become Stage 8.5 exit requirements after PS-3; adding one to the
required path needs an explicit roadmap decision.

The dependency and ownership order is recorded in [the Stage 8.5 roadmap](roadmap.md).
The PS-1 baseline suites and their authorities are frozen in the
[testing inventory](testing-inventory.md).

## Boundary with Stage 9

Stage 8.5 may add:

- ordinary same-tick structured loops;
- bounded or fuel-limited dynamic iteration;
- structs, owned lists, strings, and the representations they require;
- typed book/item access;
- the smallest safe Minecraft macro boundary required for runtime values to enter
  command syntax; and
- synchronous command-limit proofs and diagnostics.

Stage 8.5 does not add:

- persistent continuation frames;
- transparent yields;
- a runtime job queue;
- dynamic creation or cancellation of scheduled jobs;
- preservation of `@s`, position, rotation, dimension, or anchor across ticks; or
- a claim that an arbitrary Brainfuck program completes in one command sequence.

Those remain Stage 9 concerns. Stage 8.5 programs that need deterministic semantic
termination use an explicit finite fuel contract. The current target analysis may
still report `NoFiniteBoundProven` for a data-dependent cycle; strict deployment
enforcement is recorded as deferred breadth rather than implied to exist.

## Boundary with later macro and optimization work

Minecraft function macros are a target primitive, distinct from language-level
metaprogramming. PS-2 may implement typed serialization and a closed set of macro
templates needed for runtime string/list access. It must not expose arbitrary
runtime strings as commands. General language macros and broad raw-command
interpolation remain later work.

Likewise, Stage 8.5 should implement credible baseline representations and retain
choice points, but it does not need Stage 11's global plan search, profile-guided
selection, or equality saturation.

## Cross-cutting completion rule

A capability is not complete when its syntax parses. Each accepted slice must cross
the boundaries that apply to it:

```text
semantics and diagnostics
  -> typed HIR/Core
  -> verification and deterministic printing
  -> Core evaluation where target-independent
  -> optimization None/Baseline equivalence
  -> physical planning and structured target lowering
  -> source-fixture evidence
  -> vanilla-server evidence where Minecraft defines behavior
  -> cost/limit evidence
  -> documentation and handoff
```

Unsupported pieces must fail at an owned boundary with a stable diagnostic. A
partially implemented feature must not fall through to raw commands or rely on a
test inspecting compiler-private storage.

## Evidence authorities

Stage 8.5 keeps three different authorities:

1. A Core evaluator defines target-independent execution for its supported subset.
2. Compiler fixtures establish structural and lowering facts.
3. The pinned vanilla server establishes Minecraft parser and runtime behavior.

There is intentionally no general Minecraft command/datapack emulator.

## Language and library ownership

PS-2 separately classifies core language semantics, public MDL standard-library
algorithms, sealed typed Minecraft intrinsics, and compiler-private runtime helpers.
See [the standard-library boundary dossier](ps-2/standard-library-boundary.md). This
allows known optimized mcfunction recipes without making opaque handwritten commands
the source-level semantic contract.
