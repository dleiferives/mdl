# Stage 8B — Fixed-Score Compatibility Migration

Status: **complete for the retained fixed-score compatibility ABI**

## Purpose

Stage 8B makes the realization/ABI model an independently verified physical
authority alongside the retained fixed-score construction projection, while
preserving the current target program as a compatibility oracle. It adds explicit
physical preflight and constant materialization but no dynamic activation frame.

This was the highest-risk refactor boundary: the current assignment, transfer,
symbolic, resource, construction, and reconciliation pipeline is already deeply
verified. The migration proceeded through exact bidirectional verification rather
than a broad rewrite. Stage 8 deliberately did not replace mature score-copy and
control-recipe tables with isomorphic wrappers merely to remove their old Rust
names.

## Current phase order

The current lowering request performs:

```text
Core verification
options
SemanticInventory
reachable legality / command-limit evidence
CoreAmbientAnalysis
TargetPreflight
RuntimeDemand
optional Liveness
HomeAssignment / ScoreStoragePlan
ActivationPlan / CallAbiPlan / FrameStoragePlan
PhysicalStoragePlan / RealizationPlan / CallBoundaryTransferPlan
PhysicalPreflight
EdgeTransferPlan
control recipes
ResourceInventory
LoweringPlan freeze/verify/report
construction
reconciliation
target verification and execution analysis
```

Stage 8B preserves the ordering guarantee that every target recipe is selected and
validated before resource allocation.

## Corrected preflight split

The existing `TargetPreflight` owns source-semantic Minecraft operation and run-
modifier recipes. It is constructed before realization planning and should remain
there.

Add a second product after logical home assignment and realization/ABI planning:

```text
PhysicalPreflight {
  constant_materializations,
  scalar_copy/materialization recipes,
  physical ABI transfer recipes,
  target,
  limits,
}
```

In Stage 8B its selected recipe set is small:

- Boolean constant to canonical score (`0` or `1`);
- Int32 constant to score;
- same-type score-to-score assignment; and
- existing parallel-copy scratch/cycle recipes.

Frame recipes land in 8D. The implemented physical preflight retains type-split
constant and score/frame inventories and exact local sequence/fork counts. Exact
rendered line lengths remain owned by final structured-target verification, after
resource names exist; claiming those lengths before allocation would put the check
in the wrong phase.

`PhysicalPreflight::verify` independently rebuilds expected occurrences from Core,
requirements, realizations, ABI, and selected scalar instruction recipes. Construction
may consume only this frozen product; it may not reselect based on a target command
it is about to render.

## Migration strategy

### B1 — Derive physical use requirements

Build exact indexed requirements for:

- each operand of `BoolNot`, `I32AddWrapping`, `I32AddOverflowing`, and comparisons;
- each semantic result placement and recipe temporary;
- each block-parameter edge source/destination;
- each call argument and demanded result;
- each return operand and function result slot; and
- each datapack export parameter/result.

The closed scalar recipe registry should answer accepted storage classes and read/
write timing. Do not duplicate those facts in the realization builder.

### B2 — Relate existing homes to storage

During the comparison period every `AssignedHomeId`/final `HomeId` maps one-to-one to
a logical score-storage declaration. Preserve:

- `LegacyValue`, `PinnedParameter`, `PinnedResult`, `Register`, and
  `RecipeTemporary` roles;
- exact `Bool`/`I32` type order;
- stable local and flattened allocation order;
- coalescing decisions and liveness provenance; and
- public `RegisterSlot` projection.

The new plan must be able to reproduce the old home table byte for byte.

### B3 — Relate assignments to realizations

Every existing `ValueAssignment` becomes at least one score realization. The
realization's live region comes from the existing liveness/assignment proof rather
than being guessed from raw instruction order.

`FunctionLayout::value_homes` remains as a compatibility view temporarily:

```text
value_homes[value] = unique currently selected primary score realization, if any
```

Consumers migrate to exact use→realization queries. Remove the primary view only
after no production consumer treats it as authoritative.

### B4 — Make materialization explicit

Today constants become score-setting commands through scalar lowering. Stage 8B
records the constant fact, destination realization, recipe, and placement explicitly
before construction.

The target line may remain identical. The semantic difference is compiler-internal:
the renderer is no longer where a constant silently acquires storage.

### B5 — Generalize transfer locations

Prepare the existing transfer machinery for Stage 8D without introducing frame
sources yet:

```text
TransferLocation = ScoreStorage(PhysicalStorageId) | Temporary(...)
```

The resolver still implements simultaneous same-type copies and cycle breaking.
Every move relates source and destination realizations/definitions, not only homes.

### B6 — Close authority by reconciliation

For each migration boundary the implementation:

1. build old and new products;
2. compare them independently;
3. switches consumers that need physical identity (reports, recursive spills, and
   physical preflight) to the new product;
4. retains mature fixed-score construction tables as a checked compatibility
   projection;
5. run all gates; and
6. permits removal only after no consumer remains.

The remaining projection is not a second representation decision: its homes are the
same storage declarations checked by `PhysicalRealizationPlan::verify`, and its
constructed commands remain checked by the existing plan/target reconciliation.
Future frame-field parameter/result or aggregate ABIs should migrate consumers when
they cease to be score-shaped.

The migration order was:

1. reporting/dumps;
2. plan verification;
3. scalar construction;
4. call construction;
5. edge/return construction;
6. resource inventory; and
7. public maps.

## `None` and `Baseline`

### `None`

For every existing nonrecursive fixture, `None` must preserve:

- target functions and order;
- score holder/objective names;
- command structure and rendered lines;
- datapack bytes and trace;
- ABI slot order;
- source/Core/target command correlations; and
- command-cost report.

New internal reports may add realization terminology, but no artifact change is
accepted without an explicit reviewed reason.

### `Baseline`

Baseline retains existing demand, liveness, coalescing, transfer, and control-recipe
decisions. It may satisfy multiple uses from one realization and omit dead
materializations. It does not yet choose NBT or deferred-condition alternatives.

Optimization failure remains whole-pass fallback. It cannot publish half of a new
realization plan combined with half of an old home assignment.

## Plan structure after migration

The frozen `LoweringPlan` should own:

- semantic `TargetPreflight`;
- physical `PhysicalPreflight`;
- facts/requirements/realizations;
- call ABI and activation plan;
- score storage/resources;
- instruction plans naming realization occurrences or resolved storage;
- generalized edge/call transfers; and
- complete correlation/report inputs.

Avoid storing both a realization ID and a home ID when the home can be queried from
the realization. Where construction needs only storage, retain the semantic
realization link in the plan/report so verification can prove why the storage read is
valid.

## Independent reconciliation

After construction, verify:

- every materialization recipe produced exactly its expected command shape;
- every physical read occurs at the planned target command/modifier position;
- every target write defines the planned destination realization;
- score types and Boolean normalization agree;
- command local cost/effects/outcomes agree with physical preflight;
- unused planned materializations did not disappear without a corresponding plan
  update; and
- no unplanned score copy/set was introduced during emission.

This should extend the existing constructed-recipe and command-map reconciliation,
not parse rendered text.

## Diagnostics

Keep phase-specific failures:

- requirements/realization analysis: `LoweringPhase::Planning`;
- unsupported or invalid physical recipe: `LoweringPhase::Legality` or a new
  explicitly ordered `PhysicalPreflight` phase;
- inconsistent immutable plan: `Planning`;
- construction mismatch: `Construction`; and
- invalid final target: `TargetVerification`.

Successful output must never carry a partial runnable program after any of these
failures.

## Tests

### Compatibility corpus

- Every checked-in Stage 4/5 golden.
- Complete source fixture corpus.
- Stage 7 typed `say` and unsafe-command paths.
- Stage 7.5 spatial modifier/teleport/move-by paths.
- Calls with multiple arguments/results, unused results, Bool canonicalization,
  overflowing arithmetic, CFG cycles, and parallel-copy cycles.

### Differential assertions

Compare old/new:

- assigned homes and roles;
- per-value physical availability;
- instruction operand/result storage;
- call parameter/result copies;
- edge transfer steps/scratch;
- resources and names;
- target IR, artifact bytes, trace, maps, and reports; and
- local/global cost analysis.

### Corruption

- Missing physical-preflight slot.
- Recipe or occurrence order swap.
- Constant value/Boolean normalization drift.
- Use satisfied by the wrong value in the right-typed score.
- Detached realization or target command.
- Construction command changed after preflight.

### Scale

The existing 20,000-value assignment, liveness, coalescing, transfer, symbolic, and
resource tests must remain linear-sized. Add realization/preflight counts and prove
no value×use or value×storage dense matrix.

## Gate

Stage 8B completes when the new model is authoritative, the old nonrecursive `None`
output remains the exact oracle, every physical conversion is preflight-selected,
and the old one-home assumption has no production consumer.

## Research references

- [MLIR dialect conversion materializations](https://mlir.llvm.org/docs/DialectConversion/)
- [LLVM target-independent code generator](https://llvm.org/docs/CodeGenerator.html)
- [Cranelift explicit call signatures](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md#function-calls)
- [`crates/mdl-compiler/src/lower/minecraft/api.rs`](../../../crates/mdl-compiler/src/lower/minecraft/api.rs)
- [`crates/mdl-compiler/src/lower/minecraft/preflight.rs`](../../../crates/mdl-compiler/src/lower/minecraft/preflight.rs)
- [`crates/mdl-compiler/src/lower/minecraft/edge_transfer.rs`](../../../crates/mdl-compiler/src/lower/minecraft/edge_transfer.rs)
- [`crates/mdl-compiler/src/lower/minecraft/plan/verify.rs`](../../../crates/mdl-compiler/src/lower/minecraft/plan/verify.rs)
