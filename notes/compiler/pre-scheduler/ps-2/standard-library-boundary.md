# PS-2 Language, Standard Library, Intrinsic, and Runtime Boundary

Status: **ownership model frozen by [`../ps-2-0-decisions.md`](../ps-2-0-decisions.md)**

## Why this boundary matters

Brainfuck needs arithmetic helpers, result types, stacks, zippers, parsing, and
Minecraft book access. Putting all of these in the compiler would make the language
large and rigid. Putting all of them in handwritten mcfunctions would hide meaning,
effects, cardinality, costs, and representations from the optimizer.

PS-2 therefore distinguishes four layers.

## Layer 1: Core language and IR semantics

The language owns behavior that cannot be expressed faithfully as ordinary library
code or that controls evaluation itself:

- declarations, types, name resolution, calls, and modules;
- scalar operation semantics;
- branches, same-tick loops, `break`, `continue`, and return;
- aggregate/list/string value and ownership semantics;
- effects and execution-context regions;
- target-independent verifier rules; and
- primitive operations whose semantic result cannot be derived from existing public
  operations without exposing physical storage.

These are not functions selected by a module name. They have explicit HIR/Core
representations and exhaustive compiler handling.

## Layer 2: Public MDL standard library

Algorithms and domain abstractions expressible using the language belong in MDL
source modules:

```text
std.core.option
std.core.result
std.math.byte          # if Byte begins as a checked/wrapping Int32 abstraction
std.collections.stack
std.collections.zipper
std.text.bytes
std.minecraft.book     # ergonomic wrappers over typed platform primitives
```

Likely standard-library implementations include:

- stack APIs over list tail operations;
- the two-stack zipper and tape movement;
- byte normalization helpers when ordinary arithmetic suffices;
- program/opcode data types;
- bracket-validation and parsing algorithms;
- page joining after typed pages are acquired;
- fuel/result composition; and
- reusable test/assertion helpers if they have no runtime production cost.

Standard-library MDL is compiled, verified, optimized, costed, and source-mapped by
the ordinary pipeline. It must not receive privileged access to compiler-private
scores, storage paths, frames, or raw command fragments.

## Layer 3: Typed platform intrinsics and selected recipes

Some operations have semantic meaning but require target facilities unavailable to
ordinary MDL. They are exposed through typed declarations and lower to closed
semantic operations/recipes, for example:

- list tail construction/access when `List<T>` is a primitive semantic value;
- runtime string consumption/slicing;
- obtaining item/book pages from an exact-one holder;
- structured Minecraft text/item conversions; and
- a typed macro-indexed path operation if no non-macro representation suffices.

An intrinsic is not an opaque function recognized by spelling. Its declaration is
sealed/versioned by the compiler or bundled platform library and resolves to an
explicit semantic operation ID. The checker validates its full signature and target
requirements.

Where practical, every intrinsic has a target-independent reference semantics used
by the Core evaluator and differential tests. Target recipes are then proven against
that contract on vanilla.

## Layer 4: Compiler-private runtime support

Generated helpers that implement ABIs and physical plans are not public library API:

- activation-frame push/spill/restore/pop;
- compiler-owned objectives and storage roots;
- edge-copy and return dispatchers;
- materialization/representation bridges;
- instantiated macro templates and argument frames;
- load/recovery cleanup; and
- future persistent scheduler support.

They use reserved names, are emitted only on demand, may change between compiler
versions, and cannot be imported or observed by conforming MDL programs.

## Optimization escalation ladder

Choose the least privileged implementation that preserves semantics and gives the
compiler enough information:

1. **Ordinary MDL library code.** Let existing inlining, specialization, constant
   propagation, loop, and representation work optimize it.
2. **General compiler optimization.** Recognize semantic IR patterns, not a stdlib
   function's string name, when the transformation benefits user code too.
3. **Sealed typed intrinsic.** Use when the operation needs target behavior or must
   retain high-level intent unavailable after ordinary expansion.
4. **Handwritten/generated target helper.** Use only behind the intrinsic's exact
   context/effect/outcome/cost contract and pinned vanilla evidence.

This permits highly optimized known mcfunction implementations without making an
opaque handwritten pack the semantic authority.

## Handwritten mcfunction policy

A handwritten target helper is acceptable only when all of the following are true:

- the public operation has already frozen target-independent or explicitly
  Minecraft-specific semantics;
- its arguments/results use a compiler-owned typed ABI;
- context reads/writes, cardinality, effects, success/result outcomes, and local
  command/fork costs are declared and independently verified;
- recursion/reentrancy and temporary storage ownership are defined;
- the helper passes direct pinned-server conformance cases;
- an emitted artifact maps back to the semantic operation and selected recipe;
- unsupported target profiles reject it before emission; and
- the compiler may replace it with another equivalent recipe later.

Copying public community mcfunction code into the standard library does not satisfy
these requirements by itself.

## Standard-library loading during Phase 1

Stage 7 already supports an in-memory logical package. PS-2 may bundle standard
library MDL sources as compiler-distributed logical modules and insert them through
the normal module graph. Avoid designing Stage 12's filesystem/package distribution
or stable binary interface now.

Freeze during PS-2:

- the reserved logical `std` root or equivalent;
- whether a tiny prelude is implicit (recommend minimal or none initially);
- deterministic source identity/version in diagnostics and traces;
- explicit module reachability so unused library modules do not emit resources;
- how sealed intrinsic declarations are authenticated; and
- tests compiling the library under every optimization policy.

Precompilation/caching is a later performance choice. It must preserve the same
verified semantic boundary and source diagnostics.

## Provisional Brainfuck assignment

| Capability | Initial owner |
| --- | --- |
| `while`, `break`, `continue`, arithmetic semantics | Language/Core |
| Struct/list/string value and ownership semantics | Language/Core |
| List construction/tail operations | Primitive/intrinsic with evaluator semantics |
| `Option`/`Result`-like composition | Standard library once sum representation exists |
| Wrapping byte abstraction | Standard library over frozen arithmetic, unless a primitive type is later justified |
| Stack and zipper | Standard library |
| Bracket parser and opcode state machine | Standard library/application code |
| Brainfuck interpreter | PS-3 application, never stdlib/compiler |
| Typed holder/slot/book-page acquisition | Minecraft platform intrinsic |
| Page normalization/joining | Standard library where expressible |
| NBT paths, macro frames, activation frames | Private runtime/backend |

## Tests and conformance

- Compile standard-library modules through the same source-fixture and four-policy
  pipeline as user code.
- Test algorithms against primitive reference semantics in the Core evaluator.
- Differentially compare ordinary reference/library implementations and specialized
  intrinsic recipes where both exist.
- Run pinned vanilla tests for platform intrinsics and handwritten target helpers.
- Assert unused-module and unused-runtime elimination.
- Report selected intrinsic recipe and cost in `mdl explain`; do not expose private
  helper spelling as the API.
- Add compatibility tests for sealed intrinsic declarations and reject forged or
  signature-mismatched declarations.

## Decisions to make during PS-2.0

- [x] Freeze the four-layer ownership model.
- [x] Select explicit imports under logical root `std` with no implicit prelude.
- [x] Use reachable compiler-distributed virtual source modules through the ordinary pipeline.
- [x] Use compiler-synthesized closed intrinsic IDs with exact signature/version/target checks.
- [x] Classify each PS-2 capability in the PS-2.0 decision record.
- [x] Require evaluator reference semantics for target-independent intrinsics.
- [x] Freeze typed ABI/effect/cost/reentrancy/vanilla requirements for target helpers.
- [x] Require scalar/no-import fixtures to prove unused support emits nothing.
