# Stage 4: First Core-to-Minecraft Lowering Plan

Status: **Implemented and validated on official Java 26.2**

Execution checklist: [`stage-4-todo.md`](stage-4-todo.md)

Stage 4 builds the first real compiler path:

```text
verified Core SSA
    -> deterministic Minecraft layout and physical homes
    -> legal Stage 3 MinecraftProgram
    -> deterministic datapack + trace
    -> official vanilla Java 26.2 execution
```

This is a deliberately narrow vertical slice. It establishes the architectural
seam and calling/control-flow conventions without trying to solve final register
allocation, runtime strings, general storage layout, macros, scheduling, or the
source language.

## Outcome

Given a complete verified `CoreProgram`, a `SourceContext`, and explicit lowering
options, Stage 4 will produce:

- one complete verified Stage 3 `MinecraftProgram`;
- one small public lowering map for entry resources, parameter homes, and result
  homes plus the baseline activation/command-limit contract, produced through a
  richer transient legalization plan;
- a deterministic human-readable lowering dump;
- a phase-aware failure containing diagnostics and any completed non-runnable
  lowering report, never a partial target program; and
- enough public read-only lowering-map information for an integration harness to set an
  entry function's arguments and observe its results.

The first accepted program must contain real SSA computation, a function call, a
value-producing branch, a join through a block parameter, and a return. The emitted
pack must execute correctly on the pinned vanilla server.

## What the repository already establishes

Core already provides:

- dense owner-local `FunctionId`, `BlockId`, `InstId`, and `ValueId` identities;
- `Bool` and wrapping-semantic `I32` types;
- block parameters rather than implicit phi nodes;
- constants, wrapping/overflowing add, comparisons, Boolean negation, and calls;
- ordered blocks/instructions and explicit jump/branch/return/unreachable
  terminators;
- provenance on every declaration, block, instruction, block parameter, and
  terminator;
- complete verification, CFG/reachability/dominance/use analyses, and stable dumps.

Stage 3 already provides:

- validated target names and a closed Java 26.2 target;
- typed score, data, execute, call, return, raw, function, and tag operations;
- immutable declare-then-define target programs;
- structural verification and exact deterministic rendering;
- in-memory datapack and physical-line trace emission;
- a real-server installation and conformance boundary.

Stage 4 must make layout and lowering decisions. It must not teach the Stage 3
emitter about Core SSA.

## Cross-compiler findings

The design was compared with compiler families that have materially different
frontends, runtimes, and targets. The recurring patterns are more useful than any
single compiler's exact class hierarchy.

| Compiler | Relevant design | Stage 4 consequence |
| --- | --- | --- |
| GCC | Target-neutral GIMPLE SSA is lowered into target-oriented RTL; phi meaning becomes explicit machine movement before final emission. | Preserve Core as the semantic optimization level and make score moves/control resources explicit before Stage 3. |
| Go | Sequential SSA IDs avoid maps; a distinguished `lower` pass rewrites generic operations; register allocation later shuffles merge edges into expected locations. | Use dense `HomeId`/block-indexed tables, exhaustive operation lowering, and one edge-transfer resolver plus an independent symbolic checker without mutating Core. |
| Swift | Swift-specific SIL remains distinct from LLVM IR generation, and mandatory canonicalizing transforms are separated from optional SIL optimization. | Do not contaminate Core with Minecraft syntax; keep Stage 4 correctness/legalization mandatory and defer profitable alternative selection to Stage 5. |
| Rust | Backend collection determines what receives code; MIR codegen then freezes ABI facts before constructing a per-function `FunctionCx` that borrows MIR, ABI, backend blocks, and local mappings. | Verify broadly but generate only the chosen executable CFG; freeze one shared function ABI before a short-lived per-function lowering context constructs bodies. |
| GHC | STG lowers through a C-like Cmm layer, with dumps and linting available at multiple boundaries. | Keep a readable lowering dump and verify both input Core and output Minecraft IR. |
| OCaml | Machine-independent Cmm and target-oriented Mach/selection are separate compiler stages. | Keep representation/layout decisions explicit instead of hiding them inside serialization. |
| Cranelift | SSA block parameters carry edge arguments and physical locations are separate; its WebAssembly backend discussion also cautions against forcing a structurally different target through a register-machine backend abstraction. | Treat every Core edge as a simultaneous assignment, but keep Minecraft as its own lowering path rather than imitating a conventional machine backend interface. |
| LLVM and regalloc2 | Phi elimination/block parameters become concrete edge moves; cycles require a parallel-move resolver and temporary location; regalloc2 validates moves by abstract interpretation. | Resolve each Core edge once while planning, store its finalized physical transfer beside the resource decision that consumes it, and verify value flow without rerunning the resolver. |
| LLVM GlobalISel | Translation, operation legalization, register-bank choice, and instruction selection are distinct responsibilities even though they share MIR storage. | Keep semantic translation, mandatory Minecraft legalization, physical-home choice, and later cost-based encoding selection conceptually separate without manufacturing a pass framework for the baseline. |
| MLIR | Dialect conversion requires explicit legality; One-Shot Bufferize analyzes before rewriting but exposes one authoritative decision state; LLVM lowering shares one calling convention across definitions and calls. | Freeze a conservative plan containing the decisions construction will query, consume it during construction, retain only a report, and keep Stage 3 emission mechanical. No shadow schedule database, partially legal target, or independently rebuilt signature escapes. |
| Zig | AIR is a semantic per-function input to codegen, uses indexed tagged instructions with separate payload storage, keeps wrapping addition distinct, and separates legalization from liveness. | Preserve Core semantics, use closed role/tag enums plus dense identity tables without repeating structurally implied data, and keep Minecraft algorithms out of Core. Do not add another editable IR without a concrete consumer. |
| Binaryen | Its main IR stays close to the semantic target, while specialized Stack IR exists only for optimizations that need the physical stack form. | Stage 3 remains the only target IR; add a new physical IR later only if coalescing/layout optimization cannot be represented by the plan. |
| Erlang/BEAM | The compiler has SSA assertions at named pipeline points and validates generated BEAM code. | Give lowering decisions exact fixture tests and retain the final vanilla validator as execution authority. |
| .NET RyuJIT | Lowering explicitly exposes control-flow and register requirements before allocation/codegen, with strong attention to near-linear compile time. | Inventory resources and physical needs once, then emit in stable linear traversals; guard against accidental repeated whole-CFG scans. |
| HotSpot C2 | A heavily shared ideal graph is effective for global value reasoning but carries substantial machinery. | Do not introduce a sea-of-nodes or e-graph representation for the first legal lowering. Core SSA already supplies the needed semantics. |

Primary references are listed at the end of this document.

## Central architectural decision: private legalization plan, not another general IR

Stage 4 needs a short-lived authoritative `LoweringPlan`, because generated names,
score homes, edge helpers, and call slots must be known before Stage 3 declarations
are defined. It does not need another general mutable instruction representation,
and the full plan should not become public API.

```text
CoreProgram (immutable, verified)
    |
    | legality + deterministic conservative physicalization
    v
LoweringPlan (private, immutable decisions, no command text)
    |
    | declare resources -> DeclarationMap (ephemeral Stage 3 IDs)
    |
    | Core + plan + declarations construct definitions
    v
MinecraftProgram (immutable, verified Stage 3 IR)
```

The plan records decisions that must be inspectable:

```text
LoweringPlan {
  target
  namespace
  pack_abi: PackAbi
  load: PlannedFunctionId
  init_try_create: PlannedFunctionId
  load_tag: FunctionTagResourceId
  homes: dense HomeId -> Home
  target_functions: dense PlannedFunctionId -> PlannedFunction
  functions: dense Core FunctionId map
}

Home {
  holder: FakeScoreHolder
  role: HomeRole
}

HomeRole =
  Value { function: Core FunctionId, value: Core ValueId, type: CoreType }
  | Result { function: Core FunctionId, result_index, type: CoreType }
  | EdgeTemporary { function: Core FunctionId }

PlannedFunction {
  resource: FunctionResourceId
  origin: OriginId
  role: PlannedFunctionRole
}

PlannedFunctionRole =
  Load
  | InitTryCreate
  | Block { function: Core FunctionId, block: Core BlockId }
  | BranchHelper { function: Core FunctionId, edge: BranchEdge }

PackAbi {
  register_objective
  init_sentinel: StoragePath
  activation: ActivationContract::SingleContextNonReentrant
}

FunctionLayout {
  diagnostic_name_hint
  abi: FunctionAbi
  block_functions: Core BlockId -> optional PlannedFunctionId
  value_homes: Core ValueId -> optional HomeId
  parallel_copy_temp: optional HomeId
  edge_transfers: Core BlockId -> optional EdgeTransfer
}

FunctionAbi {
  entry_block: Core BlockId
  parameters: ordered HomeId
  results: ordered HomeId
}

BranchEdge {
  source: Core BlockId
  arm: Then | Else
}

EdgeTransfer =
  Jump { steps }
  | Branch {
      then_edge: BranchTransfer
      else_edge: BranchTransfer
    }

BranchTransfer {
  steps: ordered MoveStep
  helper: optional PlannedFunctionId
}

MoveStep {
  destination: HomeId
  source: HomeId
}
```

This is deliberately equivalent to a conservative always-copy analysis state:
every executable value has a unique home and every required boundary transfer is
made explicit. Later optimization may replace these choices, but Stage 4 does not
need liveness, coalescing, or a policy trait to describe a single baseline.

The plan owns its validated target names and the small amount of copied Core
metadata required by `dump_lowering()`; it does not borrow `CoreProgram` or
`SourceContext`. The returned output is therefore self-contained. Core IDs remain
correlation keys, while name hints stay diagnostic-only and never affect identity.

The register objective and activation restriction live once in `PackAbi`, because
they constrain the shared generated register file rather than any one Core function.
Each physical fake holder and its semantic role are owned exactly once in a dense
`HomeId` table. Each generated function resource, origin, and role are likewise
owned once in a dense `PlannedFunctionId` table. Block functions, initialization
functions, and edge
helpers reference that table rather than cloning names. Value maps, function ABI
slots, cyclic-copy temporaries, and `MoveStep`s
reference typed `HomeId`s rather than cloning names or full `ScoreRef`s.
`FunctionLoweringCx::score_ref` resolves a home and pairs its holder with the pack
objective only when constructing an owning Stage 3 command. The small public
boundary map may own complete `RegisterSlot` values for convenience.

`HomeRole` is the sole source of a home's Core type and ownership. Value maps and
ABI lists contain only `HomeId`; lowering and the public report query the home table
instead of copying `(CoreType, HomeId)` pairs. `EdgeTemporary` is intentionally
untyped because one scoreboard scratch slot may service independent Boolean and
integer transfers at different times. `PlannedFunctionRole` similarly makes dumps,
provenance checks, and reverse ownership direct rather than reconstructed by
searching every layout.

`FunctionAbi` is the sole calling-convention authority. Its `entry_block` resolves
through the function's block-function map rather than cloning the entry resource.
Its parameter homes are the same `HomeId`s recorded for the entry block parameters.
Call lowering writes those slots, return lowering writes its result slots, callers
read those result slots, and the public map is derived from the same record. Do not
independently reconstruct a signature or physical holder in any of those paths.

Detached or unreachable Core entities have no executable resource/home entry.
Allocated identity is retained in dumps, but target output follows verified attached
reachable layout only.

Finalized edge transfers are small physical `MoveStep` lists inside the frozen
plan. They are legalization decisions, not another program IR: they contain no
commands, effects, mutable CFG, or independently editable operations. Construction
only translates them to Stage 3 score commands. Do not add a generic dialect trait,
operation registry, mutable target CFG, serialized plan format, or a second schedule
table.

`edge_transfers` is a dense terminator-shaped block vector. `None` means the block
has no outgoing edge (`return` or rejected `unreachable`); a jump with zero moves
remains `Jump { steps: [] }`. A conditional edge owns both its ordered moves and its
optional helper resource, making disagreement between helper inventory and transfer
unrepresentable. A helper exists exactly when that edge has at least one move.
The vector position already identifies the source block, and the `then_edge` or
`else_edge` field identifies the arm, so `BranchTransfer` does not repeat a
`BranchEdge` that could disagree with its container. Construct that ephemeral key
only when naming or checking a helper's `PlannedFunctionRole`.

Planning derives each simultaneous edge assignment and resolves it exactly once.
The resolver initially uses an internal symbolic `Scratch` location. If any result
mentions `Scratch`, planning allocates the function's one copy-temporary `HomeId`,
substitutes it into every affected step, and only then freezes the plan. Thus every
stored `MoveStep` contains final `HomeId`s; neither verification nor construction
reruns cycle analysis or allocates names. Use dense vectors and closed matches, not
`HashMap<BranchEdge, ...>` or hash iteration.

Stage 3 internal calls use `McFunctionId`, not resource strings. Declaring the dense
planned-function table in order therefore produces one ephemeral `DeclarationMap`.
Body construction resolves every `PlannedFunctionId` through this map and never
performs string lookup. It is neither part of the returned output nor a mutable
extension of the plan.

```text
DeclarationMap {
  functions: dense PlannedFunctionId -> McFunctionId
  load_tag: FunctionTagId
}
```

This table has one shape invariant instead of duplicating the plan's nested CFG
shape: its function vector length must equal `target_functions.len()`, and each
position is declared from the resource/origin at that same planned ID. The load
tag is separate because it belongs to the Stage 3 tag identity domain. Calls,
blocks, initialization, and helpers all use the same resolution path.

Use the repository's existing `entity_id!`/`EntityVec` machinery for private
`HomeId` and `PlannedFunctionId` identities. Function, block, and value maps are
dense owner-indexed vectors; closed `enum`s represent terminator/edge shape. Keep
all plan fields private and expose narrow checked accessors returning references or
typed IDs. Stage 4 needs no trait objects, backend-generic associated types,
string-keyed construction maps, or public plan builder.

## Public lowering boundary

The initial façade should be small:

```text
lower_to_minecraft(
  core: &CoreProgram,
  sources: &SourceContext,
  options: &LoweringOptions,
) -> Result<LoweringOutput, LoweringFailure>

LoweringOptions {
  target: JavaEditionTarget
  namespace: PackNamespace  // must not be `minecraft`
  register_objective: ObjectiveName
}

LoweringOutput {
  program: MinecraftProgram
  map: LoweringMap
  private report: LoweringReport
}

LoweringMap {
  execution: ExecutionContract
  functions: Core FunctionId -> LoweredFunction
}

ExecutionContract {
  activation: ActivationContract::SingleContextNonReentrant
  command_limits: CommandLimitContract::CallerBoundedToTarget {
    max_command_sequence_length
    max_command_forks
  }
}

LoweredFunction {
  entry_resource
  parameter_homes: ordered (CoreType, RegisterSlot)
  result_homes: ordered (CoreType, RegisterSlot)
}

RegisterSlot {
  holder: FakeScoreHolder
  objective: ObjectiveName
}

ActivationContract {
  SingleContextNonReentrant
}

CommandLimitContract =
  CallerBoundedToTarget {
    max_command_sequence_length
    max_command_forks
  }

LoweringFailure {
  diagnostics: Diagnostics
  phase: CoreVerification | Options | Legality | Planning | Construction | TargetVerification
  report: optional LoweringReport
}
```

`LoweringPlan` is transient analysis/construction state. After Stage 3 verification,
consume it into a smaller immutable `LoweringReport` that retains only the target
mapping and decision records required for correlation and `dump_lowering()`. Drop
call-graph/SCC scratch, inventories, cycle-analysis scratch, finalized edge-transfer
steps, and the `DeclarationMap`. `LoweringOutput` owns the verified target program,
public ABI map,
and private report—not a still-actionable plan.

The output exposes only the read-only ABI queries needed by the integration harness.
A `dump_lowering()` method renders the report deterministically for tests and
debugging without exposing its Rust structure as public API.

Keep all public boundary fields private. `LoweringOutput` exposes `program()`,
`map()`, and `dump_lowering()`; `LoweringMap` exposes `execution_contract()` and a
fallible `function(FunctionId)` lookup; `LoweringFailure` exposes `phase()`,
`diagnostics()`, and optional deterministic lowering-dump text. The private
`LoweringReport` type and construction handles never appear in a public signature.
Do not publish these types until `lower_to_minecraft` can satisfy the whole contract.

The execution contract is deliberately honest about the vertical slice. Generated
code creates no selectors/forks itself and fixed slots require one non-reentrant
context, but a Core loop may execute a runtime-dependent number of commands. Stage 4
does not prove trip counts or partition work across ticks. The caller must keep an
invocation within the target defaults reported by `TargetSpec`; configurable hard
limit assumptions begin with Stage 5 cost analysis, while soft scheduling budgets
begin with their Stage 9 consumer.

`LoweringFailure` implements `Error` and exposes the shared `Diagnostics`. Failures
before a plan is frozen have no report. Failures after freeze consume the available
mapping into a non-runnable report so diagnostics and lowering decisions can be
preserved, but they never expose a partial `MinecraftProgram`, `DeclarationMap`, or
builder. The phase is structured data rather than inferred from diagnostic-code
prefixes.

Although Stage 3's general `PackNamespace` correctly permits `minecraft`, the
lowering façade rejects it with `lower.reserved-pack-namespace`. Generated private
functions, initialization storage, and future runtime resources must not occupy the
vanilla namespace. The separately declared `minecraft:load` function tag remains
the only generated resource there.
Parameter/result entries retain Core types so a caller can enforce the Boolean
`0/1` boundary instead of treating every scoreboard slot as an untyped integer.
`RegisterSlot` also guarantees a stable fake-player holder; exposing the more
general selector-capable Stage 3 `ScoreRef` would weaken the ABI type unnecessarily.
It is self-contained so a slot cannot be accidentally paired with another output's
objective. Only public ABI slots duplicate the objective; internal value homes keep
the plan-scoped representation described above.

Branch strategy is not a public option. Stage 4 always uses the safe return
dispatcher. Choosing between equivalent encodings is a compiler decision backed by
analysis and cost, so alternative strategies begin in Stage 5 rather than making
callers configure a global lowering algorithm.

Lowering and emission remain separate. A later top-level compilation façade can own
Core, sources, lowered output, artifacts, and traces together.

## Checked pipeline

The public lowering entry point performs these phases in order:

1. verify the complete Core program against the supplied `SourceContext`;
2. validate lowering options and target compatibility;
3. audit backend legality over the emitted call graph and every entry-reachable
   CFG, rejecting recursive SCCs and executable unsupported terminators;
4. derive stable reachable order, allocate ordinary homes, resolve every edge with
   symbolic scratch, allocate exactly the required copy temporaries/helpers through
   checked dense stores, freeze the complete function/home/ABI/transfer
   plan, and verify its invariants;
5. declare every planned Stage 3 function and tag, producing the typed
   `DeclarationMap`, before defining any body;
6. construct initialization and function bodies through exhaustive closed matches,
   resolving internal calls only through that map;
7. finish and verify the Stage 3 program;
8. consume the plan into an immutable `LoweringReport`, derive the small public
   `LoweringMap` from that report, and drop all construction-only state; and
9. return output only after every phase succeeds; on failure, discard partial
    target state and return diagnostics plus any completed non-runnable report.

This is an analysis/materialization/rewrite pipeline. Mandatory legalization is
separate from optimization: Stage 4 uses the one conservative legal choice rather
than running a search while emitting commands.

Build with a private mutable `PlanBuilder`, then consume it through a single
`finish` boundary into a field-private immutable `LoweringPlan`. Construction only
accepts `&LoweringPlan`; it cannot observe a half-inventoried plan or mutate a
decision. `finish` runs one private linear `verify_plan` check for unique/in-range
homes and planned functions, complete reachable-value coverage, absent detached
mappings, entry-parameter/`FunctionAbi` agreement,
home-role ownership/types, pack ABI ownership, exact CFG/transfer shape,
source/destination type agreement, complete simultaneous assignments, helper iff
nonempty conditional transfer, and copy temporary iff a finalized transfer uses it.
It also checks that only value homes and the owning function's `EdgeTemporary`
appear in transfer steps, and that every planned-function role agrees with its one
forward reference from load/init, block layout, or branch helper.

Transfer verification must be independent of transfer construction. Label every
input home with a symbolic original-value token, abstractly execute the finalized
`MoveStep`s, and compare every destination token with the mathematical simultaneous
Core edge assignment. Track scratch initialization and reject reads before writes.
Do not call the resolver again or require one exact valid sequence: later local
optimizations may produce a different correct schedule. This is linear in the
stored steps and follows the checker pattern used by regalloc2's move fuzzing.

An invariant failure is a `Planning`-phase compiler diagnostic, never a panic or
user-program legality error. Unit tests may construct corrupted private fixtures to
prove each verifier rule. These checks are always enabled for the Stage 4 baseline;
only measurements may justify making them debug/test-only later.

MLIR's rollback-capable conversion driver is useful when many competing rewrite
paths exist, but its own documentation notes the bookkeeping and debugging cost.
Core's vocabulary is closed and each Stage 4 operation has one baseline lowering,
so use direct exhaustive matches with an up-front legality audit—no pattern
registry, backtracking, or fallback backend.

Diagnostics use stable `lower.*` codes and deterministic Core function/block/
instruction traversal. They use Core's debug dumper for malformed input and never
attempt partial target rendering.

Core block/value identities are owner-local, while `HomeId`, `McFunctionId`, and
`FunctionTagId` domains are compilation-wide. Allocate them only through the
existing checked `EntityVec::push` boundary in stable traversal order. Translate
exhaustion according to the table being built and discard the private builder; no
partial plan or target state is observable. A separate pre-count traversal would
predict the same checked failure while duplicating work, so Stage 4 does not add
one. The completed plan gives the exact Stage 3 declaration count before target
construction. Stage 3 per-function command-ID exhaustion remains a checked build
error and is translated atomically into a lowering diagnostic.

## Deterministic generated names

Names derive only from stable dense identities and explicit options, never name
hints, hashes, filesystem state, or traversal-discovered counters.

Initial resource scheme:

```text
<namespace>:__mdl/load
<namespace>:__mdl/init/try_create
<namespace>:__mdl/f<function>/b<block>
<namespace>:__mdl/f<function>/e<source-block>_<edge-index>
storage <namespace>:__mdl/init/v0/<objective-hex> path initialized
```

The edge resource exists only when a conditional edge has a non-no-op parameter
copy and therefore cannot be expressed as one direct tail call. `edge-index` is `0`
for then and `1` for else; it is an outgoing-edge ordinal, not the destination block
ID. This keeps two edges from one branch distinct even when both target the same
block with different arguments.

Internally, never pass the numeric ordinal around. Use `BranchEdge { source, arm }`
with a closed `Then | Else` arm and convert it to `0 | 1` only in the generated-name
function.

`objective-hex` is lowercase hexadecimal of the validated objective's ASCII bytes,
not a hash. It is reversible, deterministic, and collision-free. `v0` is the
backend-initialization schema, not the Minecraft target version; bump it only when
the meaning or required actions of initialization change. This prevents an old
sentinel from suppressing setup after recompiling the same namespace with a
different register objective or initialization schema.

Initial fake-holder scheme within the exclusive register objective:

```text
#f<function>v<value>      one materialized Core SSA value
#f<function>r<result>     one function result slot
#f<function>t0            optional cyclic-edge-copy temporary
```

The compiler owns the selected objective and every holder in it as an internal
scratch register file. `register_objective` is explicit because Stage 4 does not yet
have a top-level pack linker capable of allocating collision-free external names.
The caller must dedicate it to this compiled pack; sharing it with another pack is
outside the ABI contract. Persistent source-level state must use separately planned
storage later. Name hints appear only in dumps/comments, not identity.

Every generated name is constructed through Stage 3 validated name types and has
exact uniqueness tests.

## Provenance policy

Generated resources and commands use one deterministic nearest-semantic-origin
rule:

- load/init functions, load-tag entry, objective creation, and sentinel access use
  `OriginId::UNKNOWN` as compiler-owned scaffolding;
- a block function declaration uses the Core block origin;
- an edge-helper declaration uses the source branch terminator origin;
- scalar commands and every part of a call sequence use the Core instruction
  origin; and
- edge copies, transfers, result-slot copies, and native returns use the source
  terminator origin.

This is a direct policy, not a heuristic search through operands. One lowered Core
operation may produce several physical lines, all of which retain the same semantic
origin in the existing Stage 3 trace.

## Initial physical representation

Every value defined by an entry-reachable block in every Core function receives one
unique fake-player scoreboard home. Core has no export-root declaration yet, so all
functions remain potential entry points and are planned; reachability is only
per-function CFG reachability from that function's entry block.

```text
Core I32  -> scoreboard i32
Core Bool -> scoreboard value proven to be exactly 0 or 1
```

One exhaustive private type-lowering function is used when creating value/result
`HomeRole`s; `FunctionAbi` and the public map query those roles rather than lowering
types again. Stage 4 does not need an MLIR-style extensible type converter: both
Core types use score storage, and `Bool` adds the normalized-value invariant. Adding
a Core type must make this closed match fail to compile until its representation
and ABI behavior are defined.

Constants are still materialized in this first slice. This is intentionally less
optimized than deferred constants/conditions, but it gives one simple invariant for
calls, joins, loops, and integration tests. Stage 5 may rematerialize constants,
coalesce compatible live ranges, and keep branch-only comparisons deferred.

Unique homes make correctness auditable:

- an SSA definition writes only its own value home;
- ordinary uses read that home;
- block edges write destination parameter homes simultaneously;
- calls write callee entry-parameter homes and read callee result slots;
- Boolean homes are normalized by construction;
- no physical home reuse can invalidate a branch condition in Stage 4.

The first layout is a spill-everything baseline, analogous to assigning each value a
stable stack slot before register coalescing. It is not the final performance model.

## Target-semantics preflight

Core specifies wrapping `i32` addition and an exact signed-overflow flag. Before
lowering arithmetic, an ignored vanilla test must pin:

- scoreboard `+=` at `i32::MAX + 1` and `i32::MIN - 1`;
- scoreboard assignment and swap at both boundaries;
- comparison behavior at signed boundaries;
- absent-holder behavior for every command sequence Stage 4 relies upon.

If native scoreboard addition wraps exactly, it is the primitive for both add
operations. If it does not, Stage 4 must legalize with explicit correction; it must
not weaken Core semantics.

Extend the existing ignored `mdl-test/tests/minecraft_ir.rs` conformance pack for
this preflight so all primitive facts share its one Java startup, artifact
determinism check, trace check, and log audit. Also query the current namespaced
`minecraft:max_command_sequence_length` and `minecraft:max_command_forks` gamerules
and compare them with `TargetSpec`. This preflight is an implementation gate, not an
assumption in the plan.

The Java 26.2 conformance run measured the required facts before lowering began:

- assignment, `+=`, `-=`, and swap preserve both signed boundaries, and native
  addition/subtraction wrap modulo `2^32`;
- all signed score comparisons behave correctly at `i32::MIN` and `i32::MAX`;
- scoreboard operations materialize an absent source or target score as zero and
  report success, including swap; the tests pin every resulting value rather than
  relying on implicit absence;
- `execute if function` accepts a positive return and rejects both zero and a
  failed `return run` command;
- the first successful `return run data modify ... set value` reports result `1`,
  while a missing source reports failure; and
- both command-limit gamerules default to `65_536`, matching `TargetSpec`.

These measurements permit the baseline native score sequences. Lowering must not
use missing-score failure as a guard: every planned home is still defined
explicitly, so zero materialization remains an observed target fallback rather than
part of the Core semantic model.

Stage 4 also needs one narrow Stage 3 vocabulary addition before constructing its
reload guard: an internal `execute if function` condition carrying an
`McFunctionId`. Render and verify it through the existing declaration identity,
and conservatively give it the same unknown effect/context/fork contract as an
ordinary function call; do not add interprocedural contract inference merely for
this condition. Pin its nonzero-return behavior on Java 26.2. The Stage 4 plan
separately guarantees that its generated creation probe is selector-free and
contains only objective creation. Do not use an external string target or unsafe
raw command. This is a target fact required for correct initialization, not a
general Core operation.

This ID introduces no new symbol mechanism. Thread the existing
`&MinecraftProgram` from command rendering through modifier/condition rendering,
and extend Stage 3 reference verification to walk execute modifiers as well as
nested command nodes. Dumping, rendering, contracts, verification, and tests must
match the new closed condition variant exhaustively. An absent `McFunctionId` is
the existing `minecraft.invalid-internal-reference` diagnostic.

Add the condition first, then extend the public Stage 3 handoff fixture and the
existing vanilla conformance test. Do not expose a Core-to-Minecraft entry point
until the complete plan/construction pipeline can return a verified program without
stubs.

## Scalar operation lowering

The first legal lowering is syntax-directed and exhaustive.

### Constants

```text
I32Constant(n):
  scoreboard players set <result> <objective> n

BoolConstant(false/true):
  scoreboard players set <result> <objective> 0/1
```

### Wrapping add

After the overflow preflight proves native semantics:

```text
result = left
result += right
```

Both commands use typed `ScoreCommand::PlayersOperation` so negative inputs and the
entire `i32` domain are legal.

### Overflowing add

Produce the same wrapping sum plus a normalized flag. Signed overflow occurs only
when both inputs have the same sign and the wrapped result has the opposite sign:

```text
sum = left
sum += right
overflow = 0
if left >= 0 and right >= 0 and sum < 0: overflow = 1
if left < 0 and right < 0 and sum >= 0: overflow = 1
```

Use ordered execute-condition chains and exact signed score ranges. Boundary tests
cover all four sign combinations and `i32::{MIN,MAX}`.

### Signed comparisons

Materialize normalized Boolean results:

```text
result = 0
execute if/unless score <left> <op> <right> run result = 1
```

`Eq`, `SignedLt`, `SignedLe`, `SignedGt`, and `SignedGe` map to Stage 3's native
comparison operators. `Ne` uses `unless ... = ...` rather than inventing a raw
operator.

### Boolean negation

Given the normalized-Boolean invariant:

```text
result = 1
result -= operand
```

The second command is a typed score operation. Entry Boolean parameters must be 0/1;
internal calls and generated operations preserve the invariant.

## SSA edges and parallel score copies

A Core edge's arguments are simultaneous assignments to its destination block
parameters. Sequentially emitting them is incorrect for a loop-carried swap:

```text
^header(%a, %b)
backedge ^header(%b, %a)
```

Stage 4 implements one deterministic parallel-copy resolver whose inputs are
`HomeId`s and whose temporary internal output may mention `Scratch`:

1. discard `destination == source` moves;
2. repeatedly emit any move whose destination is not used as a remaining source;
3. if only cycles remain, choose the earliest pending destination in original
   destination-parameter order, save it to `Scratch`, replace that source in the
   pending set with `Scratch`, and resume;
4. preserve stable destination-parameter order when several moves are ready;
5. never allocate names during resolution.

This resolver is used only for CFG block-argument edges. Call-argument sources and
callee entry homes have different function IDs; callee result slots use the `r`
family; and return sources use `v` while destinations use `r`. Their source and
destination address sets are therefore disjoint, so calls and returns are safe
ordered assignments and do not need the cycle algorithm.

Planning runs the resolver once per edge. If any resolved transfer in a function
mentions `Scratch`, allocate `#f<function>t0`, replace every `Scratch` occurrence
with that `HomeId`, and then freeze. No later phase detects cycles or allocates a
temporary on demand.

The baseline uses assignment operations. A two-cycle may later use native scoreboard
swap as a local optimization, but the generic resolver is the correctness authority.

Proofs include no-op edges, chains, fan-out, two/three cycles, mixed cycles and
acyclic moves, self-loop swaps, and deterministic output. In addition to named
fixtures, exhaustively enumerate small valid move graphs and small initial integer
states; executing the resolved sequence must equal mathematical simultaneous
assignment. This tests the resolver independently of Minecraft rendering.

Do not mutate Core merely to split critical edges. Go's backend creates critical
edge blocks so phi movement has an edge-local insertion point; Minecraft needs the
same semantic property only when a conditional edge expands beyond one command.
The generated helper is therefore a target-level edge adapter keyed by source and
outgoing ordinal, not a new Core block and not a blanket CFG-normalization pass.

## Baseline control-flow layout

Initially, every reachable attached Core block becomes one target function. This is
a correctness baseline, not the final layout policy.

### Jump

Emit the target edge's parallel copies in the current block function, followed by:

```mcfunction
return run function <destination-block>
```

### Return

Copy returned values into the owning function's distinct result homes in declaration
order, then emit a successful native return. The semantic return values live in
explicit ABI slots; the Minecraft function command's own success and integer result
are both the internal value one.

```mcfunction
return 1
```

This uniform ABI supports zero, one, and multiple Core results. Later lowering may
use the native function result for profitable single-result calls.

### Unreachable

Reject an entry-reachable Core `unreachable` with
`lower.unsupported-reachable-unreachable`. `return fail` is not a semantic trap: a
caller can continue and then observe stale result slots, so emitting it would hide a
missing target-level failure-propagation design. Detached or CFG-unreachable blocks
are not emitted and therefore need no target terminator.

### Branch: mandatory Stage 4 lowering

The safe return dispatcher is directly expressible inside the current block
function:

```mcfunction
execute if score <condition> <objective> matches 1 run return run function <then-edge>
return run function <else-edge>
```

The condition is tested once on either path. If an edge needs parallel copies, its
target is a generated edge helper which performs copies and tail-calls the target
block. Otherwise the branch calls the target block directly.

Snapshot and dual-guard forms are Stage 5 lowering-selection candidates. A dual
guard is legal only after proving that the first arm cannot change the second
guard's observed condition. Stage 4 deliberately has no strategy enum with one real
choice and no caller-controlled policy knob.

## Function-call ABI

Minecraft functions have no ordinary typed parameter/result ABI. The first Stage 4
ABI uses compiler-owned global score slots:

```text
caller operand homes
    -> callee entry block-parameter homes
    -> function <callee-entry-resource>
    -> callee result homes
    -> caller instruction-result homes
```

Argument and result copies preserve declaration order. Their generated source and
destination addresses are disjoint, so these are ordinary ordered assignments
rather than parallel-copy scheduling. The callee's return blocks write its result
homes before returning through the target function chain.

This ABI's activation contract is deliberately:

- internal to one compiled datapack;
- single-context, non-reentrant, and non-fork-safe;
- deterministic but not externally stable;
- valid for nested nonrecursive calls; and
- not a promise that any two generated roots may execute concurrently.

Sequential calls are valid, including nested calls whose call graph is acyclic.
Concurrent roots or invocation beneath a multi-entity execution context can race on
shared callees and scratch slots even without recursion. The pack-level public map
therefore documents `SingleContextNonReentrant` rather than implying that recursion
is the only limit or trying to expose a fragile pairwise-concurrency analysis.

Core permits recursion, but fixed global frames do not. Stage 4 treats every defined
function as a potential root, then adds call-graph edges only for calls in that
function's entry-reachable CFG blocks. Detached or CFG-unreachable calls generate no
commands and must not cause false recursion rejection.

Run a deterministic SCC analysis over that emitted graph. Report one
`lower.recursive-call-abi` diagnostic per recursive SCC, ordered by its lowest
`FunctionId`, and list member IDs in ascending order. Anchor it at the earliest
in-SCC call instruction under function/block/instruction order, which identifies an
actionable cycle edge while remaining deterministic. A singleton is recursive only
when it has a self-edge. CFG loops are unrelated and remain legal. Recursion needs a
concrete stack/frame design, likely using storage and macros, and is deferred rather
than miscompiled.

Represent outgoing and reverse call edges as dense function-indexed vectors whose
entries retain the ordered call site. Use iterative Kosaraju traversal with explicit
DFS frames, not host-language recursion over user-sized graphs. Visit roots and
neighbors in stable ID/call-site order, then sort each reported component by
`FunctionId`. This is intentionally simpler and more stack-robust than implementing
recursive or rollback-capable graph machinery for one rejection check.

The lowering dump records this ABI restriction. Generated functions use fake holders
and no selectors, so their own commands do not fork.

This does not make runtime-dependent loops compatible with an arbitrarily small
command-sequence limit. Until Stage 9 adds bound analysis and static continuation
partitioning, the public execution contract also says that callers must keep each
root invocation within the target command-sequence/fork limits. Stage 4 integration
uses small inputs and records the queried gamerules; it does not claim a general
loop bound.

## Initialization and entry boundary

Scoreboard objectives persist across `/reload`, and adding an existing objective is
a failed command. A naïve initializer that runs `objectives add` and then writes a
sentinel is wrong: `.mcfunction` execution continues after the failed add, so it
would silently adopt a colliding objective. Generate a guarded load function plus a
creation probe, and append only the load function to `minecraft:load` with
`FunctionTagMerge::Append`:

```mcfunction
# <namespace>:__mdl/load
execute if data storage <namespace>:__mdl/init/v0/<objective-hex> initialized run return 1
execute if function <namespace>:__mdl/init/try_create run return run data modify storage <namespace>:__mdl/init/v0/<objective-hex> initialized set value 1b
return fail

# <namespace>:__mdl/init/try_create
return run scoreboard objectives add <register-objective> dummy
```

The compiler owns this exact schema/objective-qualified storage ID/path as
initialization metadata. It is not a general Core storage representation. The
probe returns nonzero only when objective creation succeeds. The sentinel write is
therefore reachable only after successful creation; if either creation or commit
fails, load returns failure and leaves the sentinel absent so a later reload can
retry. An existing sentinel makes routine reloads quiet. The exclusive-objective
contract means a preexisting objective with no matching sentinel is an invalid,
observable deployment collision, never silently adopted.

Score values are created by their defining assignments. Repeated entry-function
invocation does not require clearing every value because verified SSA definitions
dominate uses and calls overwrite argument/result slots before reading them.

The public lowering map exposes an entry function's parameter and result score
homes. The vanilla test can therefore:

1. install the emitted pack into a fresh world;
2. let `minecraft:load` create the objective;
3. reload once and prove initialization is quiet and does not recreate state;
4. set normalized input homes from server console;
5. invoke the generated entry block resource;
6. query result homes exactly;
7. correlate failures through Stage 3 trace and the Stage 4 lowering dump.

At the end of the successful integration run, remove only the compiler sentinel
while leaving its objective in place. Invoke load twice while storing function
success to test storage. Both calls must fail, the sentinel must remain absent, and
the load path must invoke no Core entry. This reaches the same foreign-objective
state without paying for a second Java startup.

Stage 4 does not generate a permanent user-facing export ABI yet.

## The current mutable-state/raw gap

The roadmap's first-slice list mentions scoreboard-backed mutable state and raw
commands, but current Core has no load/store/global/raw operation. Stage 3 supports
raw target commands; that does not make raw syntax a language-independent Core op.

Do not solve this by adding `CoreOp::MinecraftRaw`.

For Stage 4:

- the real Core lowering covers scalar SSA, calls, CFG, joins, loops, and returns;
- the integration harness may use console commands to set entry homes and observe
  results;
- the one compiler-reserved storage sentinel is backend initialization scaffolding,
  not a source-visible mutable-state feature;
- Stage 5G's first profitable terminal-call contraction does not repeat a condition,
  so physical condition-stability analysis and its mutation fixture remain deferred
  with the optional dual-guard/snapshot recipes; Stage 4 emits only the return
  dispatcher;
- a later stage must design a target-neutral import/intrinsic/effect boundary before
  source-level raw commands or mutable global state enter Core.

This is a discovered vocabulary boundary, not an excuse to smuggle target text into
Core or layout metadata.

## Deterministic lowering dump

Add one verified dump for the layout and decisions, for example:

```text
target 26.2 namespace=mdl register_objective=mdl.reg
execution single-context-nonreentrant limits=caller-bounded sequence=65536 forks=65536
fn0 choose -> mdl:__mdl/f0/b0
  param[0] %v0 bool -> #f0v0 mdl.reg
  param[1] %v1 i32  -> #f0v1 mdl.reg
  result[0] i32     -> #f0r0 mdl.reg
  bb0 -> mdl:__mdl/f0/b0
  bb1 -> mdl:__mdl/f0/b1
  edge bb0.then -> direct bb1
```

The dump is not parsed and does not duplicate `.mcfunction` rendering. It explains
where values/resources went, the fixed activation contract, and why an edge helper
was required.

## Per-function lowering context

After the plan is frozen, body construction uses one private
`FunctionLoweringCx<'a>` at a time. It borrows the Core function body, its planned
homes/functions, declarations needed for calls, and the Stage 3 body builder. Scalar
and terminator helpers operate through this context, and `finish` consumes it.

This follows the useful part of rustc's `FunctionCx` pattern without copying its
backend framework. The context is ephemeral orchestration state, owns no global
mutable compiler state, cannot change the plan, and is not another IR.

## Complexity guardrails

The first lowering should be approximately linear in program size plus emitted
edge-move count:

- verify Core, the frozen plan, and final Stage 3 once each;
- derive each function's call/CFG information once;
- allocate dense maps sized from existing entity counts;
- inventory planned functions in stable traversal once;
- lower each reachable instruction/terminator once;
- resolve each edge copy set independently;
- never clone a whole function body or program;
- never render commands during lowering;
- avoid global string maps when dense typed identities suffice.
- never iterate a hash map to determine output or diagnostic order;
- use explicit work stacks for compilation-wide graph traversal.

The successful acyclic path remains linear. On an invalid recursive program only,
sorting recursive SCC members and diagnostics for stable presentation may cost
`O(R log R)` for the `R` functions involved; do not impose that sorting cost on
successful compilation.

Add a non-gating geometric benchmark over increasing straight-line functions and
CFGs. Do not introduce packed storage or an arena until measurements show the simple
layout is inadequate.

## Module boundary

Initial implementation inside `mdl-compiler`:

```text
src/lower/
  mod.rs                   options/output façade, exposed only when complete
  minecraft/
    mod.rs                 checked phase orchestration
    plan.rs                legality, call graph, names, homes, ABI, dump
    parallel_copy.rs       deterministic simultaneous moves
    build.rs               per-function context and Stage 3 construction
```

Start with these four concrete responsibilities. Split scalar/control/name helpers
only after their implementations become independently substantial. This keeps the
target direction visible without building a generic backend framework or eight
mostly empty modules up front. Implement private options/names, planning, declaration,
and construction in dependency order; do not publish a façade backed by placeholder
phases.

## Required proof corpus

At minimum automate:

1. option/name validation and deterministic generated resources/holders;
2. fresh-world one-time initialization, append-mode load-tag composition, quiet
   state-preserving repeated reload, and retryable rejection when the objective
   exists without the compiler sentinel;
3. Core verification always precedes planning;
4. detached/unreachable Core entities emit no executable target resource;
5. every Core scalar operation, including every comparison and overflowing-add
   boundary;
6. Boolean normalization for constants, negation, comparisons, parameters, and
   call results;
7. every parallel-copy shape, especially loop-carried swaps;
8. jump, return, empty block, multi-result return lowering, and deterministic
   rejection of entry-reachable `unreachable`;
9. default return dispatcher with exact target bytes;
10. the target-level condition-mutation regression proving why dual guards are not
   yet a legal lowering choice;
11. direct, forward, nested, zero-argument, zero-result, and multi-result calls;
12. deterministic rejection of direct and mutual recursion plus documentation of
    the non-reentrant/non-fork-safe and caller-bounded command-limit contract;
13. the canonical value-producing Core diamond;
14. the existing `sum_down` loop plus a separate finite loop whose backedge really
    swaps carried values and therefore requires the planned edge temporary;
15. byte-identical lowering dump, target program dump, artifact, and trace across
    repeated lowering;
16. no partial Stage 3 program on any lowering diagnostic, with no report before
    plan freeze and a preserved non-runnable report for post-freeze failure;
17. a scale test and separate geometric benchmark;
18. the extended existing Stage 3 official-server preflight for arithmetic,
    function conditions, `return run`, and current target gamerules;
19. one official-server Core-to-datapack execution through the mandatory dispatcher;
    and
20. failure preservation containing Core dump, lowering dump, artifact, Stage 3
    trace, world, and server logs.

## Explicit deferrals

Stage 4 does not implement:

- a source parser, HIR, or source-level type checker;
- source-level mutable globals, raw interpolation, or Minecraft command APIs;
- recursive/reentrant/fork-safe call frames;
- stable external ABI or separately compiled datapack linkage;
- score-home reuse, liveness-based coalescing, spilling, or representation search;
- deferred Boolean conditions or command-result/success value representations;
- snapshot/dual-guard branch selection and condition-stability proofs;
- block fusion, hot traces, join duplication, inlining, or outlining heuristics;
- general storage/NBT value allocation;
- selectors, execution context capabilities, or entity values;
- macro calls/frame ABI;
- scheduling, yielding, task queues, or command-limit partitioning;
- e-graphs, equality saturation, profile guidance, or a cost model;
- multiple Minecraft targets;
- a generic lowering/dialect framework; or
- filesystem output.

These remain later work. The baseline must be correct, transparent, measurable, and
replaceable by better policies.

## Completion gate

Fast gate:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Pinned target gate:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test core_lowering -- --ignored --nocapture
```

Stage 4 is complete only when the Core program—not a hand-authored Stage 3 substitute—
produces and executes the observed result on vanilla.

## Primary sources

- [Current GCC GIMPLE/Tree SSA internals](https://gcc.gnu.org/onlinedocs/gccint/Tree-SSA.html)
- [Current GCC RTL SSA phi-node model](https://gcc.gnu.org/onlinedocs/gccint/RTL-SSA-Phi-Nodes.html)
- [Go compiler pipeline and generic-to-machine SSA lowering](https://go.dev/src/cmd/compile/README)
- [Go compiler call-graph SCC analysis](https://go.dev/src/cmd/compile/internal/ir/scc.go)
- [Go SSA backend pass design](https://go.dev/src/cmd/compile/internal/ssa/README)
- [Go SSA critical-edge splitting for phi implementation](https://go.dev/src/cmd/compile/internal/ssa/critical.go)
- [Go register allocation and merge-edge shuffling](https://go.dev/src/cmd/compile/internal/ssa/regalloc.go)
- [Swift compiler architecture and AST/SIL/LLVM phase separation](https://www.swift.org/documentation/swift-compiler/)
- [Rust MIR-to-codegen lowering](https://rustc-dev-guide.rust-lang.org/backend/lowering-mir.html)
- [rustc reachable item collection before code generation](https://rustc-dev-guide.rust-lang.org/backend/monomorph.html)
- [rustc per-function codegen context and precomputed ABI](https://github.com/rust-lang/rust/blob/main/compiler/rustc_codegen_ssa/src/mir/mod.rs)
- [rustc terminator and call lowering](https://github.com/rust-lang/rust/blob/main/compiler/rustc_codegen_ssa/src/mir/block.rs)
- [GHC Cmm backend boundary and selectable backends](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/codegens.html)
- [GHC multi-level dumps and linting](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/debugging.html)
- [OCaml native backend sources (`Cmm`, selection, `Mach`)](https://github.com/ocaml/ocaml/tree/trunk/asmcomp)
- [Cranelift SSA, block parameters, and physical value locations](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md)
- [Cranelift discussion of a structurally different WebAssembly backend](https://github.com/bytecodealliance/wasmtime/issues/2566)
- [LLVM phi elimination implementation](https://llvm.org/doxygen/PHIElimination_8cpp_source.html)
- [regalloc2 block-parameter moves and parallel-move resolution](https://docs.rs/crate/regalloc2/0.13.2/source/doc/ION.md)
- [LLVM GlobalISel translation/legalization/location/selection pipeline](https://llvm.org/docs/GlobalISel/Pipeline.html)
- [MLIR full/partial conversion and type materialization](https://mlir.llvm.org/docs/DialectConversion/)
- [MLIR transient analysis caching and invalidation](https://mlir.llvm.org/docs/PassManagement/)
- [MLIR symbols, symbol tables, and non-SSA references](https://mlir.llvm.org/docs/SymbolsAndSymbolTables/)
- [MLIR analysis-driven and conservative always-copy bufferization](https://mlir.llvm.org/docs/Bufferization/)
- [MLIR two-stage LLVM lowering and shared calling-convention conversion](https://mlir.llvm.org/docs/TargetLLVMIR/)
- [Zig semantic AIR and separate legalization/liveness](https://github.com/ziglang/zig/blob/master/src/Air.zig)
- [Zig AIR legalization implementation](https://github.com/ziglang/zig/blob/master/src/Air/Legalize.zig)
- [Zig AIR liveness implementation](https://github.com/ziglang/zig/blob/master/src/Air/Liveness.zig)
- [Zig backend MIR/legalization direction](https://ziglang.org/devlog/2026/)
- [Binaryen target-close IR, Stack IR, local coalescing, and deterministic tools](https://github.com/WebAssembly/binaryen)
- [Erlang BEAM SSA checks](https://www.erlang.org/docs/26/apps/compiler/ssa_checks)
- [.NET RyuJIT phase design](https://github.com/dotnet/runtime/blob/main/docs/design/coreclr/jit/ryujit-tutorial.md)
- [HotSpot C2 Ideal graph overview](https://wiki.openjdk.org/display/HotSpot/Overview%2Bof%2BIdeal%2C%2BC2%27s%2Bhigh%2Blevel%2Bintermediate%2Brepresentation)
- [Minecraft Java 26.2 release and data-pack format 107.1](https://feedback.minecraft.net/hc/en-us/articles/46690753273997-Minecraft-Java-Edition-26-2)
- [Minecraft Java 1.20.3 `return run` and function-condition semantics](https://feedback.minecraft.net/hc/en-us/articles/21968446892173-Minecraft-Java-Edition-1-20-3)
- [Minecraft Java 1.21.11 namespaced command-limit gamerules](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-11)
