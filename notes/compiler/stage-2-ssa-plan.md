# Stage 2: Typed SSA Implementation Plan

Status: **Implemented**

Stage 2 builds a small verified SSA CFG for typed, ordered programs. It is the
semantic bridge between future HIR and the Minecraft IR. It does not contain
Minecraft commands, physical storage choices, source-language modules, or a general
compiler framework.

## Design corrections from the deeper review

The previous plan was still too eager in several places.

| Concern | Previous direction | Revised direction |
| --- | --- | --- |
| Types | Intern every type behind `TypeId` | Store the initial compact `CoreType` by value |
| Locations | Intern structurally equal locations | Append immutable provenance; equality is not semantic |
| Ownership | One broad mutable IR context | Shared `SourceContext`; independent `CoreProgram` |
| Entity deletion | Vacant arena slots or eventual generic removal | Append identities; detach from executable layout |
| Layout | Separate general layout subsystem | Block order plus instruction order inside each block |
| Unreachable SSA | Reject all cross-block uses | Enforce dominance only where the use is reachable |
| Mutation | Clone the whole function for every pass | Preflight local edits, mutate in place, verify after each pass |
| Instruction motion | Generic move operations | No generic motion in Stage 2 |
| Erasure | Unused results were sufficient | Operation must also be pure |
| Calls | External linkage and summaries immediately | Internal declare-then-define calls; all calls conservative |
| Function names | Unique string names are identity | `FunctionId` is identity; names are optional display hints |
| Crate boundary | A source-aware `mdl-ir` crate | One `mdl-compiler` crate with `source` and `ir::core` modules |

These changes remove infrastructure whose benefit would only appear after we have a
larger type system, cached analyses, or performance evidence.

## Semantic contract

Core IR is an ordered SSA control-flow graph.

- SSA edges describe value dependencies.
- Instruction order describes sequencing inside a block.
- Terminators describe sequencing between blocks.
- Pure operations may later be reordered when dataflow and speculation permit it.
- Unknown/effectful operations retain their relative program order.
- Every arithmetic operation defines overflow and comparison semantics explicitly.
- There is no implicit poison, undefined value, or host-dependent arithmetic.
- Block arguments represent joins and loop-carried values; there is no `Phi` op.

This distinction matters for our target. Minecraft programs mutate scores, NBT,
entities, command context, and visible output. SSA alone does not make those effects
safe to duplicate or reorder.

## Ownership model

```text
SourceContext
  files: SourceMap
  origins: EntityVec<OriginId, Origin>

CoreProgram
  functions: EntityVec<FunctionId, Function>

Function
  name_hint: Option<Box<str>>
  parameters: Vec<CoreType>
  results: Vec<CoreType>
  origin: OriginId
  body: Option<FunctionBody>

FunctionBody
  blocks: EntityVec<BlockId, BlockData>
  instructions: EntityVec<InstId, InstData>
  values: EntityVec<ValueId, ValueData>
  block_order: Vec<BlockId>
  entry: BlockId

BlockData
  origin: OriginId
  parameters: Vec<BlockParam>
  instructions: Vec<InstId>
  terminator: Option<Terminator>
```

`SourceContext` is shared by HIR and Core during one compilation. Source files and
origins therefore do not need to be cloned during lowering. `CoreProgram` owns only
Core functions; source-language packages and modules remain a HIR concern.

Stage 2 is single-threaded. If parallel function compilation later makes origin
allocation contentious, we can shard or synchronize it then. We should not put locks
through the initial API speculatively.

## Compact types, not premature type interning

Cranelift represents its common SSA value type as a compact value rather than an
interned graph node. Our Stage 2 type set is much smaller:

```text
enum CoreType {
    Bool,
    I32,
}
```

`CoreType` is `Copy`, immutable, directly comparable, and stored on each `ValueData`
and function signature. Zero function results represent a void return; there is no
fake unit SSA value.

We should introduce type interning only when recursive or parameterized types make
it useful. At that point it can be added behind a type API without changing
`ValueId`, CFG structure, or operation order. HIR may need rich types before Core
does; forcing them to share one early `TypeStore` would couple the levels unnecessarily.

## Typed append-only entities

```text
FunctionId
BlockId
InstId
ValueId
FileId
OriginId
```

Each is a private-constructor `u32` newtype. `EntityVec<I, T>`:

- allocates IDs densely and monotonically;
- never reuses an ID;
- offers typed `get`, `get_mut`, keys, and enumerated iteration;
- does not expose `usize` indexing or its raw vector publicly;
- rejects ID allocation beyond the representable range;
- iterates deterministically.

Cranelift's `PrimaryMap` documentation warns that two stores using the same entity
type create conflicting references. We accept function-local `BlockId`, `InstId`,
and `ValueId` because compact local IDs simplify every analysis. The API always
scopes these IDs through a borrowed `FunctionBody`; `FunctionId` is similarly local
to one `CoreProgram`.

A numeric ID accidentally taken from another body cannot always be detected when
the same index exists locally. Private constructors, private stores, body-scoped
builders/editors, and no global ID registry are the deliberate tradeoff. Adding a
generation or body tag to every hot ID is not justified yet.

## Identity and executable placement

Entity allocation and executable placement are distinct:

- a block is attached when it occurs exactly once in `block_order`;
- an instruction is attached when it occurs exactly once in the instruction list of
  an attached block;
- allocated but unattached entities are detached;
- detached entities are ignored by ordinary printing and analysis;
- attached code may not reference detached blocks, instructions, or values.

No entity is physically deleted during Stage 2. Detaching dead code preserves IDs
and keeps mutation simple. We record allocated-versus-attached counts so later
measurements can tell us whether an explicit compacting rebuild is worthwhile.

There is no general `Layout` object. Placement, parent block, and instruction
position are derived from the two authoritative order vectors.

## Function declaration and definition

Functions use a two-step lifecycle:

```text
declare_function(name_hint, signature, origin) -> FunctionId
define_function(function, body)
```

This permits forward calls, self recursion, and mutual recursion. A body can be
installed exactly once. Full-program verification rejects:

- an internal function that remains undefined;
- redefining an existing function;
- calls to invalid function IDs;
- calls whose operands/results do not match the declaration.

`FunctionId` is the only Core identity. `name_hint` exists for diagnostics and IR
dumps and need not be unique. Canonical output names functions from their stable IDs
(`@fn0`, `@fn1`, and so on); a readable hint can be printed as metadata or a comment.
This prevents specialization, inlining artifacts, and generated helpers from needing
a premature global symbol-mangling policy.

Stage 2 has no external linkage model. All functions are internal and all `Call`
operations conservatively have unknown effects and are not speculatable. External
datapack/library linkage belongs to a later module/ABI design.

## Values and definitions

Every value is defined exactly once:

```text
ValueData {
    ty: CoreType,
    definition: ValueDef,
}

ValueDef::BlockParam {
    block: BlockId,
    parameter_index: u32,
}

ValueDef::InstResult {
    instruction: InstId,
    result_index: u32,
}
```

Instructions store their result `ValueId`s. Block parameters store a `ValueId` and
an `OriginId`. The verifier checks both directions of these relationships.

An instruction result derives its provenance from the defining instruction. We do
not duplicate an origin on `ValueData`, where it could disagree with the definition.

Multiple results are supported from the beginning. This is useful for explicit
overflow operations now and for distinct Minecraft command result/success values in
Stage 3.

## Source spans and generated provenance

The compiler source concept is named `Origin`, not `Location`, to avoid collision
with the language's Minecraft `Location` type.

```text
Span {
    file: FileId,
    start: u32,
    end: u32,
}

Origin::Unknown
Origin::Source(Span)
Origin::CallSite {
    callee: OriginId,
    caller: OriginId,
}
Origin::Fused {
    inputs: Vec<OriginId>,
    reason: Option<Box<str>>,
}
```

`SourceMap` stores immutable source text and line-start indexes. Spans use
file-relative UTF-8 byte offsets and can be resolved to a line and byte column. A
span must be ordered, within its file, and on UTF-8 boundaries. Files larger than the
supported `u32` range produce a diagnostic. Tab expansion, grapheme width, and
terminal display columns belong to the later diagnostic renderer, not `SourceMap`.

Origins are append-only but not interned. Two structurally equal origins may have
different IDs because provenance identity has no effect on program semantics.
Compound origins may reference only existing origins, preventing cycles. Origin zero
is always `Unknown`, so every IR entity has an explicit origin without an `Option`.

Functions, blocks, block parameters, instructions, and terminators carry origins.
Giving blocks their own origin is necessary for diagnostics such as an empty block
with no terminator, where no contained instruction can supply a useful location.

## Block arguments instead of phi instructions

MLIR's block-argument form gives us one representation for function parameters,
joins, and loop-carried values:

```text
func @choose(%condition: bool, %left: i32, %right: i32) -> i32 {
^entry:
  branch %condition, ^then(%left), ^else(%right)

^then(%value: i32):
  jump ^join(%value)

^else(%value: i32):
  jump ^join(%value)

^join(%result: i32):
  return %result
}
```

The entry block parameters correspond exactly to the function parameter types. An
attached terminator may not target the entry block. Every successor edge passes the
exact number and types required by its destination.

Branch arguments are uses at the predecessor terminator, not definitions in the
successor. The successor's block parameters are separate definitions.

## Structural terminators

Rust MIR's separation between statements and terminators fits our small closed CFG:

```text
Terminator {
    kind: TerminatorKind,
    origin: OriginId,
}

Jump(BlockTarget)
Branch {
    condition: ValueId,
    then_target: BlockTarget,
    else_target: BlockTarget,
}
Return(Vec<ValueId>)
Unreachable
```

```text
BlockTarget {
    block: BlockId,
    arguments: Vec<ValueId>,
}
```

Terminators produce no SSA results. A block may temporarily have no terminator while
being constructed, but every attached block must have exactly one at builder finish
and pass verification boundaries. `Yield` remains deferred until static scheduling
defines its semantics.

## Closed operations with explicit semantics

The bootstrap operation set is deliberately small:

```text
BoolConstant(bool)
I32Constant(i32)
I32AddWrapping
I32AddOverflowing     // results: wrapping sum, signed-overflow flag
I32Compare(predicate) // predicate explicitly states signed relation
BoolNot
Call(FunctionId)
```

Each operation defines in one implementation:

- canonical printed name;
- operand types;
- result types;
- effect class;
- speculation permission;
- precise semantic description.

Named builder methods derive result types. Normal callers cannot claim arbitrary
result types. Crate-private malformed constructors exist only for verifier tests and
a future textual parser.

Arithmetic contracts:

- `I32AddWrapping` returns the low 32 bits of two's-complement addition;
- `I32AddOverflowing` returns the same sum plus `true` exactly when signed `i32`
  addition overflowed;
- comparison predicates distinguish equality from signed ordering explicitly.

## Effects and speculation

Stage 2 needs only a conservative interface:

```text
EffectClass::Pure
EffectClass::Unknown

Speculation::Always
Speculation::Never
```

Constants, Boolean operations, comparisons, and the defined total arithmetic
operations are pure and always speculatable. Calls are unknown and never
speculatable.

Unknown operations:

- cannot be erased even when their results are unused;
- cannot be duplicated;
- cannot be moved relative to other observable operations;
- cannot be replaced with a weaker effect through the generic editor.

Stage 3 expands this into Minecraft-specific reads, writes, context changes, forks,
failure, and scheduling boundaries. We do not thread a synthetic world token through
Core SSA; sequential block order already represents effect order without polluting
every function signature.

## Derived analyses

Stored IR contains only authoritative definitions, operands, block order,
instruction order, and terminators. Stage 2 derives:

```text
PlacementIndex  InstId -> BlockId and position
UseIndex        ValueId -> instruction/edge/return uses
ControlFlowGraph
Reachability
DominatorTree
```

Analyses are rebuilt on demand and never stored in `FunctionBody`, so Stage 2 has no
cache invalidation protocol. `UseSite` covers instruction operands, branch
conditions, successor arguments, and returns.

Analysis objects borrow the `FunctionBody` that produced them. Rust then prevents a
mutable editor borrow while a placement, use, CFG, or dominance view is still live.
An editor performs any required analysis during preflight, drops the analysis view,
and only then mutates. We do not need revision counters or stale-cache assertions.

## Reachability and dominance

We use the Cooper-Harvey-Kennedy immediate-dominator algorithm over blocks reachable
from entry. The public dominance query is three-valued:

```text
Dominates
DoesNotDominate
UnreachableUse
```

This prevents an optimizer from confusing “dominance was not defined for this dead
use” with a proven result.

Verifier rules:

- same-block definition-before-use order is checked in every attached block;
- a reachable cross-block use must be dominated by its definition;
- an unreachable use skips cross-block dominance checking;
- a value defined in unreachable code cannot be used by reachable code;
- definitions used by attached code must themselves be attached;
- entry is the only CFG root considered by reachability/dominance;
- attached blocks may be temporarily unreachable.

Skipping cross-block dominance for an unreachable use is intentional. After folding
a branch, a formerly reachable arm may become dead while still referring to entry
values. Verify-after-transformation should continue to work before unreachable-code
elimination runs. Optimizations either ignore unreachable blocks or remove them
first.

## Layered verification

Verification accumulates safe independent diagnostics in this order:

1. function declaration/definition and symbol integrity;
2. ID range and definition-table integrity;
3. block/instruction attachment and duplicate placement;
4. value definition/result/parameter bidirectional consistency;
5. origins and source spans;
6. operation operand/result contracts;
7. terminator structure and target validity;
8. entry, call, edge, and return signatures;
9. CFG/reachability consistency;
10. same-block order and reachable SSA dominance.

Later layers do not index through IDs that an earlier layer found invalid. The
verifier operates on fallible lookups and never assumes the builder was used.

Printing has two contracts:

- `CanonicalPrinter` accepts verified IR and emits stable snapshots;
- `DebugDumper` accepts malformed IR, uses only fallible lookups, includes detached
  entities, and prints explicit placeholders for invalid references.

Failure bundles use `DebugDumper`. Otherwise the diagnostic path for a malformed
pass could itself panic while trying to print the problem.

## Construction API

`FunctionBuilder` is body-scoped and maintains an insertion block. It supports:

- allocating and attaching blocks;
- appending typed block parameters;
- inserting named operations;
- setting each terminator once;
- querying produced values;
- finishing with function verification against the currently declared signatures.

The builder prevents obvious structural mistakes but does not replace the verifier.
Compiler transformations and a future textual parser do not necessarily use the
builder. Undefined callees are permitted during individual function construction;
full-program verification requires every internal declaration to be defined after
all bodies have been built.

## Verified in-place transformation API

Cloning every function for every pass is not an acceptable default. Cranelift's own
documentation notes that function cloning is not fast, and a long optimization
pipeline would turn that cost into repeated whole-function copying.

Instead, transformations follow this lifecycle:

1. optionally capture the before-IR when failure diagnostics are enabled;
2. complete pattern matching and local preflight before mutation;
3. apply edits in place through `FunctionEditor`;
4. verify the function after the pass;
5. stop the entire compilation pipeline if the pass or verifier fails;
6. emit the pass name, pipeline, before-IR when enabled, and invalid after-IR.

This follows MLIR's distinction between an expected non-match and pass failure. A
rewrite that is merely not applicable must not mutate. An internal pass failure may
leave invalid IR, but no later pass runs and the current compilation unit is
discarded. The compiler process and other compilation requests remain usable.

The Stage 2 failure bundle is diagnostic, not automatically replayable: its textual
IR has no parser yet. Once textual parsing or stable serialization exists, the same
bundle can become a true crash reproducer containing the input and pass pipeline.

The initial editor supports only operations it can preflight structurally:

```text
replace_value(old, new)
replace_pure_inst(old, replacement)
erase_pure_inst(inst)
set_terminator(block, replacement)
detach_unreachable_blocks()
```

Rules:

- `replace_value` requires equal types and checks dominance at all reachable uses
  before changing anything;
- pure instruction replacement requires equal result arity/types and rewrites every
  use, including edge and return uses;
- erasure requires a pure operation with no remaining result uses;
- terminator replacement validates condition and successor signatures and cannot
  target entry or detached blocks;
- unreachable detachment handles the entire unreachable subgraph and leaves no
  attached references to detached entities;
- every method completes validation before mutation, so a returned error is not a
  partial edit.

`replace_pure_inst` requires both the removed and replacement operations to be pure.
Structural checks cannot prove the two operations semantically equivalent; that is
the transformation's proof obligation. The editor protects IR invariants and effect
classification, not mathematical correctness of an optimization.

There is no generic instruction motion or effectful replacement in Stage 2. Block
fusion, scheduling, and effect-aware motion should arrive as purpose-built passes
once Stage 3 provides the information they require.

## Required invariants

1. IDs are allocated once by one owner and never reused.
2. Placement has exactly one authoritative representation.
3. Every attached block occurs exactly once in block order.
4. Every attached instruction occurs exactly once in one attached block.
5. Every value has one immutable type and one definition.
6. Result/parameter tables agree with value definitions in both directions.
7. Every attached block is terminated at verification boundaries.
8. Entry parameters match the function signature.
9. Successor and call arguments match their declarations exactly.
10. Attached code never references detached definitions or targets.
11. Reachable uses obey SSA dominance; all same-block uses obey definition order.
12. Generic erasure and replacement never discard unknown effects.
13. Expected rewrite rejection performs no mutation; internal pass failure stops the
    pipeline and produces a complete failure bundle.
14. Printing and analysis iteration are deterministic for identical IR.

## Crate structure

Stage 2 adds the compiler crate:

```text
crates/mdl-compiler/
  src/
    lib.rs
    source.rs
    entity.rs
    ir/
      mod.rs
      core/
        mod.rs
        print.rs
        builder.rs
        verify.rs
        analysis.rs
        edit.rs
        pass.rs
```

This is a navigation direction, not an instruction to create every file on day one.
Start with `entity.rs`, `source.rs`, `ir/core/mod.rs`, and focused tests. Split
further only when the code becomes difficult to navigate.

Using `mdl-compiler` avoids making the future parser, HIR, diagnostics, and driver
depend on a crate nominally dedicated only to IR. We keep one compiler crate until a
real build-time, dependency, or process boundary justifies a split. `mdl-test`
remains independent and is used only by integration tests or a higher-level driver.

## Stage 2A: Entities, compact types, and source provenance

Implement:

- typed IDs and append-only `EntityVec`;
- `CoreType::{Bool, I32}`;
- `SourceMap`, `Span`, `Origin`, and `SourceContext`;
- an empty `CoreProgram`;
- deterministic debug formatting and unit tests.

Exit criteria:

- Rust code cannot mix ID kinds;
- invalid typed lookups are fallible;
- IDs never move or get reused;
- source spans validate bounds and UTF-8 boundaries;
- compound origins reject invalid references and cannot form cycles;
- origin allocation is deterministic but does not intern equality;
- no function-body, CFG, or operation implementation exists yet.

## Stage 2B: Functions, SSA entities, and printer

Implement:

- function declaration and one-time definition;
- `FunctionBody`, blocks, instructions, values, and placement;
- block parameters and multi-result value definitions;
- structural terminators;
- bootstrap `CoreOp` semantics;
- read-only traversal;
- canonical and debug printers.

Exit criteria:

- forward, self-recursive, and mutually recursive calls are representable;
- a diamond and loop can be represented internally;
- overflowing add exposes two correctly typed results;
- detached entities retain identity and are absent from canonical output;
- debug output can reveal detached entities;
- repeated output is byte-identical.

## Stage 2C: Builder, contracts, and local verifier

Implement:

- `FunctionBuilder`;
- centralized operation contracts;
- verifier layers through calls/edges/returns;
- structured diagnostics with origins;
- full-program verification for declared/defined functions.

Exit criteria:

- normal builders cannot lie about operation result types;
- valid diamond, join, call, and loop examples verify;
- malformed raw test IR reports several independent diagnostics safely;
- missing definitions and redefinition are rejected;
- unknown-effect calls cannot be erased by generic helpers;
- verifier failures never panic.

## Stage 2D: Uses, CFG, reachability, and dominance

Implement:

- placement and use indexes;
- successors, predecessors, DFS postorder, and RPO;
- reachability;
- Cooper-Harvey-Kennedy immediate dominators;
- three-valued dominance queries;
- reachable SSA and all-block local-order verification.

Exit criteria:

- a reachable sibling-branch value leak is rejected;
- a join block argument is accepted;
- valid loop-carried values verify;
- branch simplification may leave a dead arm without invalidating reachable SSA;
- unreachable-to-reachable value flow is rejected;
- detached entities are absent from every analysis;
- live analysis objects prevent mutable body access through Rust borrowing;
- results are deterministic.

## Stage 2E: Verified in-place editing

Implement:

- named in-place pass runner with verify-after-pass;
- same-type/dominance-safe value replacement;
- pure instruction replacement and erasure;
- validated terminator replacement;
- unreachable subgraph detachment;
- optional before-IR capture and failure bundle;
- invalid after-IR diagnostics;
- deterministic generated edit sequences for invariant stress tests.

Exit criteria:

- an expected rejected editor operation leaves the body byte-identical;
- a verifier failure stops the pipeline before another pass runs;
- failure output identifies the pass and contains the pipeline plus before/after IR;
- effectful unused calls cannot be erased;
- replacement covers ordinary, edge, and return uses;
- dominance-invalid replacements fail before mutation;
- unreachable cleanup leaves no attached references to detached code;
- generated edit sequences either return controlled errors without mutation or
  produce verified IR;
- no editor test panics.

## Stage 2F: Proof corpus and Stage 3 handoff

Add canonical snapshots and verifier tests for:

1. value-producing `if/else` with a block-argument join;
2. nested Boolean branches;
3. a loop with counter and accumulator block arguments;
4. overflowing arithmetic with multiple results;
5. forward, recursive, and mutually recursive calls;
6. unknown-effect call ordering;
7. branch folding followed by unreachable cleanup;
8. invalid reachable cross-branch use;
9. source, call-site, and fused origins;
10. a deliberately failing in-place transformation and its failure bundle.

Stage 3 receives read-only APIs for attached block order, instruction order,
operations, operands, results, types, effects, successors, and origins. It does not
reach into entity storage.

Exit criteria:

- every valid proof function passes full verification;
- every invalid proof function fails for the intended reason;
- snapshots are deterministic;
- strict formatting, Clippy, tests, and docs pass;
- no Minecraft selector, score, NBT path, execution context, or command appears in
  Core IR.

## Explicitly deferred

- source parser, source type checker, and HIR module semantics;
- type interning and recursive/parameterized Core types;
- dynamic dialect or operation registration;
- textual IR parser and stable serialization;
- analysis caching and invalidation;
- generic instruction motion;
- effectful-operation rewriting;
- physical entity compaction and ID remapping;
- fine-grained Minecraft effects and alias analysis;
- poison/undefined-value semantics;
- scheduler and yield semantics;
- e-graphs and global canonicalization;
- external package/datapack ABI.

## Implementation result

Stages 2A through 2F are implemented in `crates/mdl-compiler`. The implementation
record and verification commands are in
[`stage-2-ssa-implementation.md`](stage-2-ssa-implementation.md). Stage 3 can now
build its Minecraft-aware IR and lowering against Core's read-only APIs.

## Primary sources

- [MLIR language reference: SSA values, blocks, and block arguments](https://mlir.llvm.org/docs/LangRef/)
- [MLIR side-effects and speculation rationale](https://mlir.llvm.org/docs/Rationale/SideEffectsAndSpeculation/)
- [MLIR pattern-rewriter mutation discipline](https://mlir.llvm.org/docs/PatternRewriter/)
- [MLIR builtin source-origin forms](https://mlir.llvm.org/docs/Dialects/Builtin/)
- [Cranelift compact SSA `Type`](https://docs.rs/cranelift/latest/cranelift/prelude/struct.Type.html)
- [Cranelift dense primary entity storage and identity warning](https://docs.rs/cranelift-entity/latest/cranelift_entity/struct.PrimaryMap.html)
- [Cranelift data-flow graph and value definitions](https://docs.rs/cranelift-codegen/latest/cranelift_codegen/ir/dfg/struct.DataFlowGraph.html)
- [Cranelift unreachable-code detachment](https://docs.rs/cranelift-codegen/latest/src/cranelift_codegen/unreachable_code.rs.html)
- [Cranelift reachable-block dominator implementation](https://docs.rs/cranelift-codegen/latest/src/cranelift_codegen/dominator_tree/simple.rs.html)
- [Cooper, Harvey, and Kennedy's dominance algorithm](https://hipersoft.cs.rice.edu/grads/publications/dom14.pdf)
- [rustc typed-index collection rationale](https://doc.rust-lang.org/stable/nightly-rustc/rustc_index/vec/struct.IndexVec.html)
- [rustc MIR blocks and structural terminators](https://rustc-dev-guide.rust-lang.org/mir/index.html)
- [rustc source maps and byte positions](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/source_map/index.html)
- [Rust structure-aware fuzzing guidance](https://rust-fuzz.github.io/book/cargo-fuzz/structure-aware-fuzzing.html)
