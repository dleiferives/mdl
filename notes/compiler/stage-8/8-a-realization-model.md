# Stage 8A — Physical Realization and Activation Model

Status: **complete for the scalar Stage 8 boundary**

## Purpose

Stage 8A adds the vocabulary and immutable analysis products needed to describe where
an existing scalar Core value is physically available and how a call activation may
reuse storage. It does not yet change command emission.

The core correction is:

```text
one ValueId != one permanent representation != one physical home
```

## Non-goals

- No structs, lists, strings, entity handles, runtime positions, or new source syntax.
- No deferred-condition optimization.
- No target resource allocation or rendered command fragments.
- No recursion/fork legality change.
- No global representation search.
- No general alias/reference system.

## Current implementation to migrate

Today `HomeAssignment` gives each demanded scalar value one `AssignedHomeId` and
`LoweringPlan::FunctionLayout::value_homes` exposes one optional final `HomeId` per
value. The plan separately owns:

- `InstructionPlan` scalar operand/result homes;
- `FunctionAbi` parameter/result homes;
- call argument/result destinations;
- `EdgeTransferPlan` parallel copies;
- liveness/coalescing results; and
- `HomeRole`/assigned-home provenance.

Those structures already have strong ownership and verification. The implemented
migration therefore retains them as a score-compatibility projection, but makes the
owned physical plan independently reconstruct and verify every projected storage,
realization, use, ABI slot, and recursive bridge. Neither table may silently disagree
with the other. Removing the compatibility projection is cleanup for a later ABI
migration, not a scalar-correctness prerequisite.

## Domain model

### Facts

`ValueFacts` is an analysis product derived from Core, initially only:

```text
Unknown
ConstantBool(bool)
ConstantI32(i32)
```

Facts are not allocated, do not have target identity, and may satisfy a use only
through a target recipe that accepts an immediate or through a materialization.
Existing scalar command recipes generally require scores, so constant materialization
remains explicit.

Do not add `DeferredCondition` here. A condition's validity depends on the effectful
program point and Minecraft context, not only the semantic value.

### Storage classes and identities

The initial closed storage classes are:

```text
PhysicalStorageClass =
  ScoreBool
  | ScoreI32
  | ActivationNbtBool
  | ActivationNbtI32
```

Planned storage identity should remain typed and program-owned. Two reasonable Rust
shapes are either one `PhysicalStorageId` into a closed declaration table or distinct
`ScoreStorageId`/`FrameStorageId` IDs behind a closed `StorageRef` enum. Prefer the
shape that makes foreign-ID mistakes impossible without forcing every consumer to
switch on irrelevant variants.

Every declaration records:

- class and semantic `CoreType`;
- owner function/SCC/frame schema;
- allocation role;
- origin/provenance;
- whether it is reusable scratch, pinned ABI, or frame-owned;
- a deterministic local ordinal; and
- later, its allocated target home/path.

At Stage 8A these are logical storage identities. Score holder names and NBT paths
are allocated only after physical preflight.

### Realizations

Use a dedicated occurrence identity:

```text
RealizationId

RealizationDecl {
  value: ValueId,
  storage: PhysicalStorageId,
  ty: CoreType,
  definition: RealizationDefinitionSite,
  live_region: RealizationLiveRegion,
  origin: OriginId,
}
```

Definition sites initially cover:

- function parameter entry;
- scalar instruction result;
- block-parameter/edge transfer destination;
- call result;
- constant materialization; and
- future frame load.

Do not store only an instruction index interval: Core liveness already supports CFG
segments. Reuse or adapt its sparse function-local representation so loops and
branches remain correct and scale linearly.

`FunctionRealizationPlan` contains:

- dense optional realization lists per allocated `ValueId`;
- indexed exact use requirements;
- storage declarations;
- selected materializations; and
- a compatibility projection back to the current value-home API while migration is
  incomplete.

One value can have several realization IDs. One storage can appear in several
realizations only when their live regions do not overlap.

### Use requirements

Requirements belong to physical operand occurrences, not types alone:

```text
UseRequirement {
  semantic_value: ValueId,
  site: PhysicalUseSite,
  accepted: nonempty ordered set<PhysicalStorageClass>,
  timing: ReadTiming,
  origin: OriginId,
}
```

Sites include:

- scalar instruction operand/index;
- CFG transfer source;
- call argument/index;
- return value/index;
- semantic operation operand when added; and
- export ABI boundary.

The accepted set is derived from a closed selected recipe/ABI contract. It is not a
free-form preference attached by an optimizer. In the first migration almost every
ordinary scalar use accepts exactly `ScoreBool` or `ScoreI32`.

`ReadTiming` remains necessary because simultaneous recipes and parallel transfers
must not allow an output to overwrite an input before its last physical read.

### Materializations

Stage 8A declares identities and contracts; Stage 8B selects the first recipes:

```text
MaterializationOccurrence {
  value: ValueId,
  source: FactOrRealization,
  destination_realization: RealizationId,
  site,
  recipe,
  origin,
}
```

A materialization produces a new equivalent realization. It does not change the
semantic value and is not represented as a Core instruction.

### ABI and activation

Keep separate plan products:

```text
AbiValueMode = ElidedKnown | DirectScore | ActivationFrameField

FunctionAbiPlan {
  parameters: indexed semantic value + mode,
  results: indexed semantic result + mode,
}

CallOccurrencePlan {
  callee,
  argument modes/transfers,
  result modes/transfers,
  origin,
}

ActivationDiscipline = SerialStatic | RecursiveStack
```

An ABI mode references requirements/realizations but does not decide whether two
invocations overlap. `ActivationPlan` is produced by a separate analysis and checked
against each ABI occurrence.

## Product and phase ownership

Recommended products:

```text
RealizationRequirements  // derived, verified, target-recipe requirements
ScoreStoragePlan         // logical score homes projected from home assignment
ActivationPlan           // overlap/SCC discipline and external entry contract
CallAbiPlan              // function signatures and exact call occurrences
FrameStoragePlan         // recursive frame fields required by activation/ABI
PhysicalStoragePlan      // merged logical score and frame storage
RealizationPlan          // selected logical storage/realizations/materializations
CallBoundaryTransferPlan // simultaneous restore/result/argument transfers
PhysicalPreflight        // target-validated physical recipes, added in 8B
```

`ScoreStoragePlan` initially projects existing `HomeAssignment`. Activation and ABI
planning consume those actual homes plus liveness and SCC information, choose the
recursive spill protocol, and produce `FrameStoragePlan`. The two storage
inventories are merged before realizations and call-boundary transfers are selected.
This avoids both possible cycles: spills are never chosen before the score homes
they protect, and final realizations are never frozen before the frame fields they
may use exist. All products are immutable after construction and retain the
optimization policy and Core identity counts against which they were built. No
product stores borrowed pointers into another phase.

`LoweringOutput` should eventually expose a compact owned read-only projection, not
the mutable planning graph.

## Construction algorithm

For Stage 8A, build the canonical existing-score plan only:

1. verify Core and reuse `SemanticInventory`, demand, and liveness;
2. derive one exact score-class requirement for every retained use;
3. create logical score storage corresponding to the current assigned homes;
4. create realizations matching current value assignments/live segments;
5. create direct-score ABI modes matching current pinned parameters/results;
6. classify synchronous activations as `SerialStatic` or `RecursiveStack` from the
   iterative overlap/SCC analysis;
7. declare an exact typed activation field for each caller home live across each
   recursive call; and
8. verify the new products against Core, assignment, liveness, activation, and the
   old authoritative plan in both directions.

This deliberate shadow/comparison period is temporary. Stage 8B flips authority only
after exact agreement.

## Independent verification

The verifier recomputes rather than trusts:

- allocated Core identity counts and dense function order;
- type of every semantic value;
- retained/demanded use occurrences;
- allowed storage classes for every selected scalar recipe;
- dominance and sparse liveness of the realization satisfying each use;
- no overlapping realizations in one physical storage;
- exact parameter/result arity and types;
- call/callee signature agreement;
- activation discipline compatibility;
- every materialization's source equivalence and destination definition; and
- absence of detached storage, realization, requirement, or ABI occurrence IDs.

Verification must be iterative and bounded. A malformed foreign ID produces a typed
diagnostic, never indexing panic.

The implemented non-overlap audit groups realizations by storage, sorts their sparse
block-local live segments, and performs a linear sweep within each group. Distinct
homes therefore verify linearly, while actual shared homes are checked without a
dense value-by-value matrix. Exact occurrence audits independently reconstruct use,
materialization, and call sets and then validate their timing, destination, origin,
recipe, and transfer correlations.

## Limits

Add explicit derived or configured limits for:

- storage declarations;
- realization occurrences;
- physical use requirements;
- materialization occurrences;
- ABI positions/call occurrences;
- liveness segments scanned; and
- verifier work.

Admission is all-or-nothing at whole occurrence/table boundaries. No partially
optimized realization plan may escape after a limit.

## Diagnostics and dumps

Stable diagnostic families should distinguish:

- missing/foreign semantic value;
- unsupported use requirement;
- no reaching realization;
- realization/type mismatch;
- overlapping storage lifetime;
- ABI arity/type/mode mismatch;
- activation/ABI incompatibility;
- detached occurrence; and
- analysis resource limit.

The dump should show facts, logical storage, realizations/live segments, uses and
their satisfying realization, ABI positions, and activation discipline separately.
Do not print a misleading single `value -> representation` line.

## Tests

- Generated straight-line/diamond/loop CFGs compared with an independent dense
  reaching-realization oracle.
- Same value used at several occurrences and same storage reused by several
  nonoverlapping values.
- Corruption of every ID, type, owner, live segment, use link, ABI mode, and policy.
- Unreachable/detached allocated history remains represented but never considered
  live runtime work.
- 20,000-value chains, wide joins, calls, and sparse liveness segments without host
  recursion or dense value×storage matrices.
- Deterministic construction under forced hash collisions/permuted nonsemantic input.

## Gate

Stage 8A completes when the new products exactly describe the existing lowering,
independently verify, scale sparsely, and cause no target/artifact change.

## Research references

- [Rust MIR Place/Rvalue/Operand lowering](https://rustc-dev-guide.rust-lang.org/mir/construction.html)
- [Rust `FnAbi` pass modes](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_target/callconv/enum.PassMode.html)
- [Swift SIL direct/indirect and exploded lowering](https://download.swift.org/docs/assets/generics.pdf)
- [Swift ownership SSA verifier](https://forums.swift.org/t/sil-ownership-model-proposal-refreshed/16872)
- [MLIR interfaces](https://mlir.llvm.org/docs/Interfaces/)
- [LLVM register-bank mappings and repair cost](https://www.llvm.org/docs/doxygen/RegBankSelect_8cpp.html)
- [`crates/mdl-compiler/src/lower/minecraft/assignment.rs`](../../../crates/mdl-compiler/src/lower/minecraft/assignment.rs)
- [`crates/mdl-compiler/src/lower/minecraft/plan.rs`](../../../crates/mdl-compiler/src/lower/minecraft/plan.rs)
