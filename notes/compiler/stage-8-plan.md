# Stage 8 Scalar Realization and Synchronous Calling Convention

Status: **complete for the frozen scalar synchronous scope**

Stage 8 should solve one bounded problem well: make the existing scalar language's
physical values and synchronous calls explicit enough to support many-context run
bodies and recursion without putting Minecraft carriers into semantic Core.

It should not simultaneously introduce structs, lists, strings, floats, persistent
entity handles, runtime coordinates, macros, or a global representation optimizer.
Those are clients of this foundation and need their own plans after the foundation
is executable.

## Revision outcome

The first draft was too broad and centered the wrong abstraction. It proposed one
`ValueRepresentation` per Core SSA value and then mixed into that enum:

- compiler-known constants;
- rematerializable/deferred conditions;
- score and NBT storage;
- aggregate layouts;
- activation-frame fields; and
- future context/entity representations.

That is not one domain. One immutable semantic value may be known at compile time,
available in a score, copied into an activation frame, and later restored into a
different score. Conversely, a frame field is a mutable storage location that holds
different semantic values over time. A deferred condition is a computation that may
be rematerialized, not a storage carrier.

The corrected design separates facts, values, storage, realizations, ABI transfer,
and activation lifetime. It also makes the first Stage 8 server feature scalar call
safety rather than speculative aggregate syntax.

## Exact Stage 8 scope

Stage 8 adds no required source type or expression syntax. It operates on the
existing `Bool`/`Int32` Core and unlocks two already represented source programs:

1. a typed run body invoked synchronously for several selected entities; and
2. direct or mutually recursive scalar functions.

The stage supplies:

- explicit per-use physical realization requirements;
- zero-or-more physical realizations of one semantic value;
- explicit typed materialization/copy recipes;
- an independently verified call ABI plan;
- a serial static activation discipline for acyclic calls and synchronous forks;
- a compiler-owned stack discipline for recursive call components; and
- exact public activation, depth, cost, and failure contracts.

The current fixed-score ABI remains the compatibility baseline and fast path.

## Six domains that must stay separate

### 1. Semantic value

A Core `ValueId` is an immutable typed SSA value. Core continues to describe
meaning, dominance, calls, block parameters, and results. It never names a score,
storage path, frame, target recipe, or Minecraft execution context.

Source writable locals are already converted to SSA updates. When explicit `mut`
parameters are implemented with aggregates later, HIR should retain source-place
permission and lower caller-visible updates to semantic inputs/results. Core does
not need a general address type merely because the source has writable places.

### 2. Proven value fact

A fact such as `Constant(7)` or a future rematerializable predicate says how a value
can be reproduced or reasoned about. It does not reserve runtime storage and is not a
physical realization.

Stage 8 initially needs only existing scalar constant facts. Deferred Boolean
computations remain a follow-on optimization because their legality also depends on
effect, context, stability, and dominance.

### 3. Physical storage

A storage identity is a mutable target location with a lifetime and type contract:

```text
StorageClass = ScoreBool | ScoreI32 | ActivationNbtBool | ActivationNbtI32
```

Concrete planned storage has a typed ID, owner, allocation role, and live interval.
Score homes and activation-frame fields are storage. Compiler constants and
conditions are not.

### 4. Physical realization

A realization states that one semantic value is available in one storage location
over a specific live interval:

```text
Realization {
  semantic_value: ValueId,
  storage: PhysicalStorageId,
  type: CoreType,
  definition_site,
  live_region,
  provenance,
}
```

One value may own zero realizations when all uses fold, one realization in the usual
case, or several equivalent realizations across calls. Storage reuse ends one
realization before another begins. Since Core values are immutable, two simultaneous
realizations require equivalence proof but no write-back coherence protocol.

### 5. Use requirement and materialization

Each retained operand occurrence states which storage classes its selected target
recipe accepts. A use is satisfied by a dominating compatible realization or by an
explicit selected materialization:

```text
ConstantI32 -> ScoreI32
ScoreI32 -> ActivationNbtI32
ActivationNbtI32 -> ScoreI32
ConstantBool -> ScoreBool
ScoreBool -> ActivationNbtBool
ActivationNbtBool -> ScoreBool
```

Materializations have typed occurrence IDs, origins, effects, costs, placement, and
target command correlations. No conversion happens implicitly inside rendering.

### 6. Call ABI and activation lifetime

The call ABI answers how semantic parameters/results cross one call edge. The
activation discipline answers whether physical storage may be reused by another
invocation. They are related products, not one enum.

```text
AbiValueMode = ElidedKnown | DirectScore | ActivationFrameField

ActivationDiscipline = SerialStatic | RecursiveStack
```

A function signature lowering and each call occurrence retain exact indexed modes
and transfers. Activation analysis separately proves which invocations can overlap
logically through nesting or recursion.

## Lessons applied from other compilers

### Rust: values, places, and ABI are different layers

Rust MIR explicitly distinguishes `Place`, `Rvalue`, and `Operand`; its codegen then
classifies ABI arguments independently as ignored, direct, paired, cast, or indirect.
MDL should preserve the same separation. A source writable place is not an NBT path,
and an indirect ABI field does not change the semantic parameter type.

### Swift: directness and ownership are independent conventions

Swift SIL can explode one formal tuple into several lowered values and separately
states whether each value is direct/indirect and owned/borrowed/inout. The important
lesson for MDL is not to encode caller-visible mutation, physical splitting, and
storage ownership in one representation choice. Later aggregate work should have a
semantic value/`mut` contract first and physical scalarization second.

### MLIR: analyze first, materialize explicitly, limit the first problem

MLIR One-Shot Bufferization separates whole-function analysis from rewriting and
uses explicit materialization operations at representation boundaries. It is also
deliberately limited around recursive function boundaries. MDL should copy the
discipline, not the scope: first freeze a complete analysis product, then build the
physical plan; do not pretend recursion and aggregate bufferization are one easy
pass.

### LLVM GlobalISel: mappings are local and repairs are real

Register-bank selection chooses mappings for instruction operands and accounts for
repair copies. This is closer to MDL than assigning one global carrier to every SSA
value. A score-accepting arithmetic use and an NBT-frame call boundary may require
two realizations plus explicit transfers.

### Zig: result destinations and backend liveness are contextual

Zig result locations allow a surrounding destination to guide construction, while
its self-hosted backend combines AIR liveness with backend-specific MIR generation.
MDL should eventually allow a result to be built directly in an accepted destination,
but Stage 8 must first represent that as a use/definition constraint. It should not
turn a source-level result-location idea into one permanent physical carrier.

### GHC/OCaml: specialized workers sit behind semantic wrappers

GHC worker/wrapper and OCaml Flambda unboxing create specialized internal entry
shapes only after demand/use information justifies them. GHC's Cmm calling convention
then assigns each lowered argument to a register or stack position. MDL should retain
its public semantic function identity, select internal physical call conventions
late, and add wrappers only when a different internal activation protocol needs one.

### Cranelift: every call has a known verified signature

Cranelift represents the function signature and calling convention explicitly at a
call. MDL already has typed Core signatures; Stage 8 adds an exact physical signature
and occurrence plan rather than letting call emission rediscover parameter homes.

## Corrected lowering pipeline

The existing lowering plan is already MDL's target-specific physical planning layer.
Do not add a duplicate general-purpose IR unless ordered physical actions become
impossible to verify in the plan.

```text
verified optimized Core
  -> SemanticInventory
  -> reachable legality + CoreAmbientAnalysis
  -> SemanticTargetPreflight (existing typed operations/modifiers)
  -> RuntimeDemand + Liveness + ActivationOverlapAnalysis
  -> RealizationRequirements
  -> HomeAssignment + logical ScoreStoragePlan
  -> ActivationPlan + CallAbiPlan + logical FrameStoragePlan
  -> merged PhysicalStoragePlan + RealizationPlan
  -> CallBoundaryTransferPlan
  -> PhysicalPreflight (materialization/frame/call recipes)
  -> generalized CFG transfers + control recipes
  -> ResourceInventory
  -> immutable LoweringPlan
  -> construction
  -> independent reconciliation and target verification
```

The existing `TargetPreflight` may keep its public/internal name during migration.
Architecturally it is semantic preflight. The new `PhysicalPreflight` is a separate
typed product because its inputs do not exist until realization and activation
planning. Both finish before resource allocation.

The frozen `LoweringPlan` should own the final realizations and ordered physical
actions. Logical score-home assignment necessarily precedes activation and
recursive-spill selection because spill sets refer to the actual score homes they
protect. Activation planning can then introduce logical frame fields. The score and
frame inventories are merged before final realizations and call-boundary transfers
are selected; target resource names still come later. Existing `value_homes`,
`InstructionPlan`, `FunctionAbi`, call result destinations, and edge transfers are
migrated rather than bypassed.

## Realization planning policy

Stage 8 does not implement a general representation search algorithm. Its candidate
set is intentionally tiny and legality mostly determines the result.

1. Derive a use requirement from every retained scalar recipe, CFG transfer, call
   input/output, function return, and export boundary.
2. Reuse a compatible dominating realization whose storage remains live.
3. Otherwise select the one canonical legal materialization recipe.
4. Allocate storage only after all requirements and materializations are complete.
5. Verify every use against the frozen realization reaching that exact occurrence.
6. Reject analysis-limit exhaustion without retaining a partial plan.

`None` preserves the current eager score plan for nonrecursive programs. `Baseline`
may reuse homes and remove provably redundant transfers. Both policies use the same
stack protocol when recursion makes static storage illegal; optimization policy must
not determine whether a valid source program is supported.

Global layout search, profiles, alternative aggregate layouts, and equality graphs
remain Stage 11 work.

## Activation analysis

### Synchronous forks are not recursion

The first draft assumed any many-context run body required an NBT activation stack.
That is probably unnecessary. If Java completes one child function invocation before
starting the next selected context, the invocations are serial and may safely reuse
the same static homes. They multiply work but do not overlap live activations.

Stage 8 must measure this directly on Java 26.2 with nested calls, values live around
calls, multiple entities, empty matches, failure/return behavior, and compiler-
generated output. If proven, the existing at-most-one run-scope gate becomes an
activation-overlap check plus the already separate fork-limit check.

### Recursive SCCs are the first dynamic-frame client

Direct and mutual recursion create genuine nested activations of the same physical
functions. Analyze the reachable direct-call graph into SCCs using the existing
iterative machinery:

- acyclic functions and edges use `SerialStatic`;
- recursive SCC entry/internal edges use `RecursiveStack`; and
- unsafe opaque operations never fabricate a call edge to a private worker.

Do not put every function on the stack merely because one SCC is recursive.

### Minimal recursive frame

Use a compiler-private command-storage tail list. The current frame is `[-1]`; after
pushing a child, its caller is `[-2]`.

```snbt
{frames:[{scc:2,function:7,arguments:{},results:{},spills:{}}]}
```

For each recursive call occurrence:

1. append a typed empty child frame;
2. copy indexed arguments and only score realizations live across the recursive edge
   into child-frame fields;
3. invoke the recursive worker under the unchanged Minecraft execution context;
4. restore live caller realizations and place demanded results through one verified
   call-boundary transfer plan;
5. remove the child frame; and
6. continue.

Values already resident in the caller frame use static `[-2]` paths and need not be
round-tripped through an unrelated score merely to be preserved. The physical
preflight owns every concrete path/schema and recipe.

The incoming transfer is simultaneous semantic assignment, not a handwritten copy
order. Extend the existing parallel-copy/edge-transfer machinery with typed frame
sources so coalesced result destinations, restored spills, and scratch homes cannot
silently overwrite one another. Liveness must reject a result destination that
aliases a different caller value still live after the call.

### Entry wrappers, cleanup, and failure contract

Recursive workers write results into their current frame. Generated wrappers keep
frame cleanup outside semantic body/branch helpers so an ordinary source `return`
cannot skip the caller's pop/restore sequence.

The Minecraft command-sequence limit can still abort the entire root before cleanup.
World writes already performed are not transactional, and the compiler must not
claim otherwise. Before implementation, Stage 8.0 must choose and test an explicit
external contract:

- recursive exports are root-only and reset compiler-private frames on entry; or
- an explicit recovery entry resets a poisoned runtime after abnormal termination.

Silently clearing frames on every exported call is not valid if exported calls may
nest. Internal source calls to a function that is also exported must target its
internal worker/activation entry, never its public root-reset wrapper.
Stable/nestable cross-pack ABI remains Stage 12.

Recursion depth is `CallerBounded` unless a future analysis proves a bound. The
public execution contract reports that fact alongside command-sequence assumptions;
Stage 8 does not invent a runtime trap or silently truncate recursion.

Scheduling/yielding with a live synchronous activation is forbidden. Persistent
continuation frames have different lifetime and ownership rules and belong to Stage
9.

## Implementation tranches

### 8.0 — Contract and Java evidence

Detailed dossier: [`stage-8/8-0-contract-and-evidence.md`](stage-8/8-0-contract-and-evidence.md).

- Measure serial versus interleaved child-context function execution.
- Measure recursive tail-frame operations, nested return/failure, and limit-abort
  residue.
- Freeze external root/recovery and caller-bounded depth contracts.

### 8A — Realization and activation model

Detailed dossier: [`stage-8/8-a-realization-model.md`](stage-8/8-a-realization-model.md).

- Add typed storage, realization, use-requirement, materialization, ABI-mode,
  activation-plan, and occurrence identities.
- Build immutable dense analysis/plan products with limits, dumps, and independent
  verification.
- Keep Core and source/HIR behavior unchanged.

### 8B — Score compatibility migration

Detailed dossier: [`stage-8/8-b-score-compatibility.md`](stage-8/8-b-score-compatibility.md).

- Express existing `value_homes`, scalar instruction operands/results, CFG transfers,
  and fixed function ABI as score realizations and exact use requirements.
- Preserve nonrecursive `None` output as the compatibility oracle.
- Add `PhysicalPreflight` before resources.

### 8C — Serial many-context activation

Detailed dossier: [`stage-8/8-c-serial-contexts.md`](stage-8/8-c-serial-contexts.md).

- Replace the blanket run-scope cardinality rejection with the measured serial-
  activation proof plus existing fork/command-limit evidence.
- Run real compiler-generated many-entity bodies containing locals and nested calls.

### 8D — Recursive SCC activation

Detailed dossier: [`stage-8/8-d-recursive-frames.md`](stage-8/8-d-recursive-frames.md).

- Select recursive frame layouts, materializations, wrappers, exact spills, and
  cleanup/recovery resources.
- Remove recursion rejection only when a complete verified stack plan exists.
- Prove direct, mutual, branching, early-return, and unused-result recursion.

### 8E — Hardening and handoff

Detailed dossier: [`stage-8/8-e-hardening-and-handoff.md`](stage-8/8-e-hardening-and-handoff.md).

- Publish exact physical ABI/activation/realization/materialization/cost explanations.
- Differentially test all policy combinations, corruption classes, limits, scale,
  determinism, and the official server.
- Write a separate follow-on aggregate/representation-client plan from the evidence
  gathered here.

## Explicitly deferred

- Deferred Boolean/command-success forms are computations and need their own
  stability/effect-aware plan.
- Structs and lists need source value, mutation, copying, ownership, and operation
  semantics before physical layout selection. They are not Stage 8 exit criteria.
- Persistent entity references need exact-one acquisition plus absence, unloaded,
  and death semantics.
- Runtime spatial values need numeric-domain and command-syntax materialization
  decisions.
- Strings/text, floats/fixed point, enums, maps, general references, and
  Minecraft-backed schemas are later vertical slices.
- Higher-order/indirect calls need typed callable identities and callback ABI work.
- Tail-recursion elimination and loop conversion are optimizations after the stack
  convention is correct.
- Scheduling/persistent continuations are Stage 9.
- Function macros and arbitrary dynamic paths are Stage 10.
- Global/profile-guided representation search is Stage 11.
- Stable cross-package ABI and nested external entry are Stage 12.

## Exit criteria

Stage 8 is complete when:

- Core contains no physical carrier, frame, or target ABI identity;
- facts, storage, realizations, ABI modes, and activation disciplines are separately
  represented and verified;
- every retained scalar use names a reaching compatible realization or explicit
  materialization;
- a semantic value can be proven across score -> frame -> score realizations;
- existing nonrecursive output retains its compatibility oracle;
- many-context run bodies safely reuse static storage under measured serial execution;
- direct and mutual recursion execute through frames only for recursive SCCs;
- abnormal termination and caller-bounded depth are reported honestly;
- unused frames, wrappers, transfers, objectives, and storage paths are absent;
- all fast, MSRV, scale, corruption, determinism, and pinned-server gates pass; and
- the next aggregate/value-feature stage starts from evidence rather than speculative
  representation variants.

## Primary references

- [Rust MIR construction: Place, Rvalue, Operand, and temporary](https://rustc-dev-guide.rust-lang.org/mir/construction.html)
- [Rust compiler ABI pass modes](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_target/callconv/enum.PassMode.html)
- [Swift SIL type lowering and direct/indirect conventions](https://download.swift.org/docs/assets/generics.pdf)
- [Swift SIL ownership/value verification model](https://forums.swift.org/t/sil-ownership-model-proposal-refreshed/16872)
- [MLIR dialect conversion and materialization](https://mlir.llvm.org/docs/DialectConversion/)
- [MLIR One-Shot Bufferization](https://mlir.llvm.org/docs/Bufferization/)
- [MLIR ownership-based buffer ABI](https://mlir.llvm.org/docs/OwnershipBasedBufferDeallocation/)
- [LLVM GlobalISel register-bank selection and repair costs](https://www.llvm.org/docs/doxygen/RegBankSelect_8cpp.html)
- [Zig result-location semantics](https://ziglang.org/documentation/master/#Result-Location-Semantics)
- [Zig backend AIR liveness and MIR generation](https://ziglang.org/devlog/2025/)
- [GHC worker/wrapper and demand analysis](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/using-optimisation.html)
- [GHC Cmm register/stack argument assignment](https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/src/GHC.Cmm.CallConv.html)
- [OCaml Flambda unboxing and specialized workers](https://ocaml.org/manual/4.14/flambda.html)
- [Cranelift function signatures and calling conventions](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md#function-calls)
- [Mojang Java function/fork/return behavior](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-3)
- [MDL native carrier inventory](../mcfunction/native-value-carriers.md)
- [MDL representation research](../mcfunction/representation-selection.md)
- [MDL frame research](../mcfunction/lists/callbacks-and-frames.md)
- [Stage 7.5 handoff](stage-7-5-handoff.md)
