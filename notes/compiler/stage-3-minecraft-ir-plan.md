# Stage 3: Structured Minecraft IR and Datapack Emission Plan

Status: **Complete**

Live execution checklist: [`stage-3-todo.md`](stage-3-todo.md)

Stages 3A through 3F are complete. Validated target atoms feed an immutable target
program with dense typed IDs, checked builders and contracts; layered verification
feeds one-pass rendering, deterministic artifacts, and trace maps; the full initial
vocabulary passes the official Java 26.2 server gate. The frozen Stage 4 boundary is
recorded in [`stage-3-handoff.md`](stage-3-handoff.md).

Stage 3 adds a verified target representation for Minecraft Java 26.2 and a
deterministic datapack emitter. It does not lower Core SSA. Stage 4 will choose
function boundaries, physical value locations, and branch shapes, then construct
this IR.

```text
Core SSA                                      Stage 2
    |
    | layout + physical allocation            Stage 4
    v
linear Minecraft functions and resources      Stage 3 IR
    |
    | syntax-directed serialization
    v
datapack files + compiler-side trace map       Stage 3 emitter
```

The target IR is intentionally close to `.mcfunction`. It preserves structure only
where types prevent invalid output or retain a distinction needed by later lowering:
resource kinds, score cardinality, NBT syntax, execute context, result versus
success, internal references, and function/tag resources.

## Evidence and reproducibility

The design was checked against:

- the implemented Core IR, provenance, verifier, and proof corpus;
- all existing `notes/mcfunction` experiments;
- the official Java 26.2 release, data-pack format 107.1, and command changes;
- the official server's generated `commands.json`, `datapack.json`, and registry
  reports;
- the unobfuscated 26.2 server implementations of function parsing, identifiers,
  tag dependency sorting, NBT paths, and pack metadata;
- Mojang's documented function outcome, return, fork, macro, pack-format, and
  heterogeneous-NBT behavior.

Generate the official reports with the pinned Java 25 runtime:

```sh
java -DbundlerMainClass=net.minecraft.data.Main \
  -jar /path/to/minecraft_server.26.2.jar --reports
```

The generated datapack report identifies `function` as a stable resource with
`mcfunction` elements and tags. The singular 26.2 paths are:

```text
data/<namespace>/function/<path>.mcfunction
data/<namespace>/tags/function/<path>.json
```

Inspection of `CommandFunction.fromLines` also establishes two target facts that
must have direct tests:

- each logical function command is limited to 2,000,000 Java `String` code units;
- a backslash as the last non-whitespace character continues the command on the
  next physical line.

Inspection of `TagLoader.tryBuildTag` shows resolved tag contents are accumulated in
an insertion-ordered set. Declared order matters, but duplicate resolved functions
are not repeated execution.

Local binary inspection is reproducible evidence, not a substitute for conformance
tests. Every relied-upon parser boundary must be exercised against the pinned server.

## Crate and ownership boundary

Keep the work inside `mdl-compiler`:

```text
crates/mdl-compiler/src/
  diagnostic.rs               shared finding/collection types from Core
  target/
    mod.rs                    closed target identity and facts
    java_26_2.rs              the only initial target
  ir/
    core/                     existing Stage 2 IR
    minecraft/
      mod.rs                  narrow exports and shared constants
      program.rs              program, functions, tags, references, IDs
      builder.rs              declarations and safe body/tag construction
      command.rs              command node/kind, calls, returns, raw escape
      selector.rs             closed selectors and cardinality
      score.rs                score operands and commands
      names.rs                validated target atoms
      nbt.rs                  SNBT values and static paths
      data.rs                 typed storage commands
      execute.rs              modifiers, conditions, stores, context
      contract.rs             conservative effects/context/fork summaries
      verify.rs               layered verifier
      render.rs               exact command sink and UTF-16 accounting
      dump.rs                 malformed-safe deterministic dump
  datapack/
    mod.rs                    in-memory artifact and trace map
    emit.rs                   checked program -> exact target files
```

Add a separate writer module only if both the harness and a future CLI need the same
safe directory-writing behavior. Initially, `mdl-test` can install the in-memory
artifact through its existing sandbox boundary. `mdl-compiler` must not depend on
the server harness.

Keep these submodules private. Re-export only deliberate safe constructors, IDs, and
read-only views from `ir::minecraft`; expose diagnostics and checked emission through
the crate/datapack façade. Avoid glob re-exports. Family modules own their data and
local constructors; `verify.rs` orchestrates global checks, `contract.rs` owns
summary types/composition, and `render.rs` remains the one exact textual backend.

The only new normal dependencies are `serde` with `derive` and `serde_json`, used by
private output-only DTOs. Do not enable `Deserialize` or expose serialized compiler
IR. Minecraft command and SNBT rendering remain dependency-free target code.

Before adding Minecraft diagnostics, extract the existing target-neutral
`Diagnostic` and `Diagnostics` containers from `ir::core::verify` into
`crate::diagnostic`. Preserve Core re-exports so Stage 2 callers do not churn.
Verifier-specific accumulator helpers stay crate-private. Minecraft and datapack
findings use stable `minecraft.*` and `datapack.*` codes; build API misuse may use a
focused `BuildError` when accumulating multiple findings adds no value.

## Cross-compiler lessons and Rust representation

The architecture deliberately borrows principles rather than data structures:

- Zig separates source-oriented, immutable ZIR from analyzed per-function AIR and
  lets code generation consume AIR without consulting the AST for semantics. Its
  current self-hosted backends then create backend-specific MIR before emission.
- GHC retains optimization-friendly Core, makes runtime behavior explicit in STG,
  then uses machine-oriented Cmm; its lint modes check invariants at several levels.
- OCaml likewise uses algebraic IRs whose variants encode the distinctions relevant
  at that level, then lowers to a Cmm form with target/runtime facts made explicit.
- Cranelift uses small typed entity references, dense primary maps, a temporary
  function builder with an explicit `finalize`, and a verifier instead of representing
  every global invariant in the host type system.
- LLVM keeps optimization and allocation concerns in `MachineInstr`, then lowers to
  the much more emission-oriented MC layer. Minecraft IR plays the MC-like role;
  Stage 4 owns the machine-like planning and legalization work.
- MLIR's useful lesson is progressive legalization: after full conversion, no
  illegal source operation remains. Its verification order also establishes local
  structure before verifiers or printers that may inspect nested operations. Its
  general operation/dialect framework is not itself needed here.
- rustc MIR keeps typed dense indexes and source information on executable nodes,
  while analyses and transforms live outside the IR definition crate. This supports
  intrinsic `OriginId`s but derived contract/graph side tables.
- Swift SIL explicitly distinguishes raw from canonical SIL because both are durable
  IR stages with different legal operations. Our pending program builder is not a
  durable IR stage, so incomplete definitions disappear at `finish` rather than
  adding a raw/canonical flag to `MinecraftProgram`.
- Nanopass design favors understandable passes with explicit input/output
  invariants over a few monolithic transforms. It does not require creating a new
  Rust enum for every trivial rewrite.

Applied here, Core remains the algebraic/SSA optimization level, Stage 4 owns the
semantic change to Minecraft execution, and Stage 3 is the fully legal target level.
Do not put Core blocks, values, or abstract branches into Minecraft IR, and do not
put textual Minecraft commands into Core. If Stage 4 becomes too large, split its
algorithm into named internal passes and analyses first; add another persistent IR
only when two adjacent passes genuinely require different invariants.

Use the existing `entity_id!` and `EntityVec` foundation rather than adding an IR
framework. This has the useful shape of Cranelift's `EntityRef` and `PrimaryMap`:
small newtype IDs index one append-only dense owner, and unrelated ID kinds cannot
be interchanged. IDs are local to their owning program or function. They are never
serialized, used as resource names, or retained across compilation units.

The Rust construction API has three layers:

1. fallible leaf constructors validate target atoms such as resource IDs,
   objectives, ranges, finite numbers, NBT paths, and raw lines;
2. `MinecraftProgramBuilder` declares resources, returns typed IDs, and defines
   each declaration exactly once;
3. scoped `FunctionBodyBuilder` and `FunctionTagBuilder` values append ordered
   contents and install one completed definition back into the program builder.

`MinecraftProgramBuilder::begin_function(id)` mutably borrows the program builder
and returns a body builder holding the exact still-empty definition slot; `push`
allocates a function-local `CommandId` from insertion order, and consuming `finish`
installs the body infallibly into that reserved slot. The tag builder has the
analogous ordered-entry API. `begin_function`/`begin_tag` reject an absent ID or a
slot already defined before returning the scoped builder. There is no public detached
`FunctionBody::new` constructor.
`FunctionBodyBuilder` is only an ordered container builder; it does not grow one
method per Minecraft command. Command-family constructors build a `CommandNode`, and
`push` accepts that common closed type.

Declaration and definition are separate because functions may be mutually recursive
and tags may contain forward references. A missing definition is invalid, while a
present empty function body is valid. Builders reject locally impossible states;
the whole-program verifier handles properties requiring global knowledge. This
keeps normal construction safe without pretending Rust types can prove tag acyclicity,
external deployment state, or cross-resource uniqueness.

`MinecraftProgramBuilder::finish` is fallible. Declaration eagerly rejects duplicate
owned resource IDs, scoped-builder creation rejects an unknown ID or second
definition, and program `finish` reports every declaration still lacking a
definition as deterministic `minecraft.undefined-function` / `minecraft.undefined-tag`
diagnostics. Its concrete result is `Result<MinecraftProgram, Diagnostics>`; local
single-action misuse continues to use `BuildError`. On success, `finish` consumes the
pending stores and produces a `MinecraftProgram` whose functions and tags contain
non-optional definitions. Temporary construction state does not leak into final IR,
the verifier, the dumper, or emission. The conversion moves definitions in dense ID
order and does not clone bodies, entries, NBT, or command payloads.

Keep closed Rust enums for target syntax. Do not add a trait object per command, a
generic visitor framework, serde-derived IR persistence, or generated command
definitions for this initial vocabulary. Exhaustive matches keep every supported
command and conservative contract visible. Exact target rendering is a dedicated
renderer; `Debug` and the malformed-safe dumper are diagnostic formats, not
serialization APIs. A leaf atom implements `Display` only when it has one canonical
textual form.

Treat `CommandKind` and its closed child enums as the single operation inventory.
Local verification, contract derivation, dumping, and rendering use exhaustive
matches with no wildcard arm over target syntax, so adding a variant fails compilation
until every consumer is updated. This captures the useful single-source-of-truth
pressure of MLIR ODS without adding TableGen or a Rust code generator for a small
vocabulary.

GHC's phase-indexed STG is useful when several passes retain the same syntax but
change which annotations are present. Stage 3 has only one such shape, so do not add
`MinecraftProgram<Phase>` or typestate parameters to every node. The checked emission
entry point is enough to prevent unchecked output. If later passes truly reuse the
same node shape with different mandatory annotations, add a phase parameter then.

Likewise, rustc records `Body.phase` because inlining can mix MIR bodies that have
traversed different amounts of desugaring and optimization, and Swift raw SIL permits
forms that canonical SIL and native code generation do not. Neither condition exists
inside final Minecraft IR: pending declarations live only in the consuming builder,
Stage 4 hands over fully legal target operations, and Stage 3 performs no mutating
pass sequence. Do not add a phase field, pass counter, or cached “verified” Boolean
to `MinecraftProgram`.

Use owned `Box<CommandNode>` for the single recursive child of `execute ... run` and
`return run`. The safe API's small depth bound makes this simpler than a second node
arena. Dense stores remain appropriate for functions, tags, and top-level commands
because those objects need stable identities. Revisit a node arena only if a real
transform needs shared command subtrees or arbitrary node replacement.

Command contracts are derived from `CommandKind`, never stored in the IR, so they
cannot become stale. Stage 3 remains an ordinary batch data structure. Packed
structure-of-arrays storage like Zig AIR, Salsa-style incremental queries, interning,
and arenas enter only if later profiling demonstrates a real memory or repeated-work
problem.

Take the useful part of functional compiler style without forcing persistent trees
onto every Rust representation:

- lowering borrows its input and returns a new owned output rather than mutating Core;
- `MinecraftProgramBuilder::finish` consumes the builder and exposes a read-only
  program, with no Stage 3 editor;
- analyses live in explicit side tables keyed by typed IDs instead of hidden mutable
  node fields or a global compiler context;
- named transforms have one stated precondition and postcondition and are verified at
  phase boundaries in tests and debug builds;
- temporary Stage 4 worklists, placement maps, and branch plans may be ordinary local
  data structures without becoming another public IR.

This gives deterministic data flow and makes passes independently testable, while
using Rust's owned vectors where purely persistent algebraic trees would add cloning
and allocation. A new persistent level is justified only when it represents a
durable semantic boundary, not merely an implementation step.

### Representation performance guardrails

Start with idiomatic Rust enums and dense vectors, then measure the actual shapes
before copying Zig's packed `MultiArrayList` design. Record `size_of` for the main IR
nodes. Add a deterministic server-free test that builds and emits 100,000 simple
commands and asserts counts/bytes, plus a non-gating benchmark over geometric input
sizes that reports throughput without a flaky wall-clock pass threshold. The
implementation must be linear in references plus output bytes except for explicit
compound-key and final-file sorting:

- builders append and offer `with_capacity` where callers know counts;
- duplicate and alias checks use lookup tables, not repeated scans;
- verification and rendering borrow the IR and never clone a whole function/program;
- tag cycle analysis is `O(V + E)` and iterative;
- each command is target-rendered once;
- files are sorted once after construction.

If profiles show enum padding dominates, first box only the cold large payloads. Move
to tag/data/extra arrays or interning only when measurements show that indirection and
more complex accessors repay their maintenance cost. Compile-time speed is a feature,
but data-oriented storage is an optimization decision rather than an IR semantic.

## Closed Java 26.2 target

The only constructible target is:

```text
JavaEditionTarget::V26_2

game version                    26.2
data-pack format                [107, 1]
function directory              function
function-tag directory          tags/function
default max command sequence    65_536
default max command forks       65_536
max logical command length      2_000_000 UTF-16 code units
conformance Java runtime        25
```

`TargetSpec::for_target(JavaEditionTarget::V26_2)` returns immutable facts. There is
no public constructor and no arbitrary `supports_x` flag combination. When another
Minecraft version is added, its match arm and conformance corpus make differences
explicit.

Stage 3 exposes the target defaults and classifies direct fork shapes, but has no
configured sequence/fork budget because it cannot yet prove a whole root's sequence
length or selector cardinality. Layout, bounded work, and configurable scheduling
budgets enter with their first consumer in Stage 9. The emitter never changes
gamerules.

The exact metadata is:

```json
{
  "pack": {
    "description": "<JSON-escaped plain string>",
    "min_format": [107, 1],
    "max_format": [107, 1]
  }
}
```

For post-82 formats `supported_formats` is removed and `pack_format` is unnecessary.
Exact min/max values avoid claiming compatibility with untested targets.

## Validated target atoms

Private-field types prevent resource kinds and command words from becoming
interchangeable strings:

```text
Namespace
PackNamespace
ResourcePath
PackResourcePath
ResourceLocation<N, P>     crate-private namespace/path pair
FunctionResourceId
FunctionTagResourceId
StorageId
DimensionId
ObjectiveName
FakeScoreHolder
NonNegativeI32
NbtKey
NbtPathKey
PackPath                   validated relative artifact path
FiniteF32
FiniteF64
ScoreRange
```

Construction is fallible and no type exposes an unchecked public string constructor.
String-backed atoms expose `as_str`; composite atoms provide a crate-private
`write_target(&mut CommandSink)` method. `Display` may delegate to the same canonical
form for diagnostics, but the emitter does not use `to_string()` as its serialization
API or allocate intermediate strings.

### Resources and artifact paths

For 26.2, namespaces use lowercase ASCII letters, digits, `_`, `-`, and `.`, while
rejecting the complete namespace `..`; resource paths additionally permit `/`.
Vanilla's low-level identifier validators accept some empty components, and command
input may supply a default namespace. Compiler IR deliberately stores an explicit,
nonempty namespace and path. `Namespace` and `ResourcePath` therefore form a
conservative subset of the command-level grammar rather than claiming to accept
every parser spelling. `PackNamespace` and `PackResourcePath` additionally prove
that a resource can map injectively into a normalized logical datapack artifact
path.

Function and function-tag IDs contain `PackNamespace` plus `PackResourcePath`;
storage and dimension IDs contain the more general command-level forms. A pack
namespace rejects the complete segment `.` or `..`. Constructing a pack resource
path adds these rules:

- reject absolute paths, backslashes, empty segments, `.` and `..`;
- reject leading/trailing/repeated `/`;
- produce `/`-separated relative paths only.

Implement `PackNamespace` as a newtype over an already validated `Namespace` and
`PackResourcePath` as a newtype over an already validated `ResourcePath`. Their
fallible conversions check only the additional artifact-segment rules; rendering and
command-level access delegate to the wrapped value. Conversion from either pack-safe
type back to its command-level type is infallible. This makes the lexical validator
single-source rather than maintaining two subtly different copies.

Mapping a validated function or tag ID into its target-specific `PackPath` is then
infallible. The mapping itself supplies `data/<namespace>/...` and the extension; no
API accepts a caller-built artifact prefix. A legitimate resource path beginning in
`data/` is not special: `example:data/foo` maps beneath the function directory as
`data/example/function/data/foo.mcfunction`.

`PackPath` proves target-relative syntax, normalization, and absence of traversal;
it does not claim that every legal Minecraft component is creatable on every host
filesystem. The in-memory artifact retains logical `/` paths. A filesystem writer
maps components without string concatenation, refuses symlink escape, and reports
host-specific failures such as reserved filenames rather than weakening the target
identifier model or silently renaming resources.

Implement the shared pair as a crate-private `ResourceLocation<N, P> { namespace,
path }`. Concrete domain types remain distinct newtypes: `FunctionResourceId` and
`FunctionTagResourceId` wrap the pack-safe specialization, while `StorageId` and
`DimensionId` wrap the command-level specialization. This shares parsing, hashing,
and rendering code without making functions, tags, storage, and dimensions type
aliases of one another. Store components rather than a second concatenated
`namespace:path` string; `write_target` writes the separator directly.

Storage and dimension identifiers never become host paths. `DimensionId` remains a
distinct wrapper even though it reuses resource-location lexical validation, so an
execution-context API cannot accidentally accept a function or storage ID. The
emitter receives validated resource identities, not arbitrary filesystem paths.

### Objectives and score holders

Objective names and score holders are not resource identifiers. In 26.2,
`ObjectiveArgument` delegates to Brigadier's unquoted-string reader. `ObjectiveName`
therefore requires one or more ASCII letters, digits, `_`, `-`, `.`, or `+`. No
historical length limit is invented; final command length remains the target bound.

The vanilla score-holder parser treats `@...` as a selector, `*` as the tracked-score
wildcard, `#...` as a fake holder, UUID-shaped text as an entity lookup, and other
literals as possible player/entity names. Stage 3 needs only stable compiler storage,
so `FakeScoreHolder` requires `#` plus the same nonempty conservative ASCII word
alphabet used above. Selectors and `*` remain separate variants. General named/UUID
holders belong to the later typed entity API or the raw escape hatch.

Share the alphabet check through one crate-private `ScoreWord` value. `ObjectiveName`
and `FakeScoreHolder` remain distinct newtypes; the latter stores only the validated
payload and renders its leading `#`. This avoids duplicated validators without
allowing an objective where a holder is required.

Stage 3 does not allocate objectives or holders. Stage 4 creates deterministic names
from stable compiler IDs; Stage 3 validates them and detects owned collisions.

### Score ranges and finite numbers

Represent the four valid range shapes directly:

```text
ScoreRange
  Exact(i32)
  AtLeast(i32)
  AtMost(i32)
  Between { min: i32, max: i32 }       invariant: min < max
```

The variants and fields are private outside the module. Named constructors make
`Between` fallible when `min > max` and canonicalize equal bounds to `Exact`, so no
value can mean an absent/absent or backwards range. Rendering is an exhaustive match
to exact, lower-open, upper-open, or closed Brigadier syntax; it never reconstructs
meaning from optional bounds or reparses a string.

`NonNegativeI32` stores an `i32` proven to be in `0..=i32::MAX`, matching the
Brigadier integer domain of scoreboard add/remove. It is not a general natural-number
type and does not silently clamp a wider input.

Floating-point command fields use finite wrappers. NaN and infinities never reach
SNBT or command rendering. Preserve signed zero. `FiniteF32` and `FiniteF64` implement
only traits whose semantics are unambiguous (`Copy`, `Clone`, `Debug`, and
`PartialEq`); do not derive `Eq`, `Ord`, or `Hash` merely for convenience. If a later
map/set consumer needs total ordering or hashing, it must first choose and test an
explicit signed-zero equivalence.

## Program and resource model

```text
MinecraftProgram
  target: JavaEditionTarget
  functions: EntityVec<McFunctionId, McFunction>
  function_tags: EntityVec<FunctionTagId, FunctionTag>

McFunction
  resource: FunctionResourceId
  origin: OriginId
  body: FunctionBody

FunctionBody
  commands: EntityVec<CommandId, CommandNode>

FunctionTag
  resource: FunctionTagResourceId
  origin: OriginId
  merge: FunctionTagMerge
  entries: Vec<FunctionTagEntry>        order is semantic

FunctionTagMerge
  Append
  Replace

CommandNode
  kind: CommandKind
  origin: OriginId
```

There is no generic string-keyed attribute bag on resources or commands. A field
that affects Minecraft semantics belongs in the relevant closed Rust type; derived
analysis data belongs in a side table; provenance remains the one intentionally
non-semantic field embedded for diagnostics and tracing. This avoids the stringly
operation problem that MLIR's declarative operation definitions are designed to
control.

Bodies are linear command lists: there are no blocks, SSA values, or implicit
fallthrough edges. Declaring every resource before defining it permits mutual
recursion and forward references without name lookup. The builder's consuming finish
step rejects missing definitions before final IR exists. An empty `FunctionBody` is a
complete definition and emits an empty file; Stage 3 does not reject syntax the loader
accepts merely because it may produce no command result.

Nested `execute ... run` and `return run` commands contain another `CommandNode`,
not a bare `CommandKind`, so verifier errors can point to the nested construct that
caused them. `FunctionBodyBuilder::push` returns the `CommandId` allocated by the
body's dense store; the ID is not redundantly stored inside the node and therefore
cannot disagree with placement. Only top-level commands have `CommandId`s because
one physical line has one trace identity. The top node's origin is line-level
provenance and may be an existing fused origin; nested origins provide more specific
diagnostics.

Internal references use typed IDs and external references use validated resource
IDs:

```text
InternalCallableRef
  Function(McFunctionId)
  Tag(FunctionTagId)

ExternalCallableRef
  Function(FunctionResourceId)
  Tag(FunctionTagResourceId)

CallableRef
  Internal(InternalCallableRef)
  External(ExternalCallableRef)
```

The verifier proves internal references exist. External references are explicit and
conservative; existence is the deployment environment's responsibility. An external
function reference whose serialized ID equals an owned function is rejected, and the
same rule applies separately to tags. Owned resources must be referenced by typed
internal ID rather than bypassing integrity checks through a second spelling. The
check is deliberately kind-specific: a function `example:foo` and function tag
`#example:foo` are distinct resources in distinct directories and may both exist.

Function tag entries preserve target order:

```text
FunctionTagEntry {
  kind: FunctionTagEntryKind,
  origin: OriginId,
}

FunctionTagEntryKind
  Internal(InternalCallableRef)          always required
  External {
    target: ExternalCallableRef,
    requirement: ExternalTagRequirement,
  }

ExternalTagRequirement
  Required
  Optional
```

Required entries may emit as strings; optional entries emit objects with `id` and
`required: false`; tag targets include `#`. Entry origins are compiler metadata and
do not enter JSON. Emit `"replace": true` only for `FunctionTagMerge::Replace`. A
helper adds an internal function to `minecraft:load` with `Append`. Verification
always rejects `Replace` for the shared `minecraft:load` and `minecraft:tick` tags;
Stage 3 has no use case that justifies erasing other packs' entries.

Owned internal references are always required by construction: the program either
contains the typed ID or is malformed. Optionality exists only on the external enum
variant, where absence is genuinely a deployment possibility. The builder exposes
separate `push_internal` and `push_external(requirement)` methods; there is no Boolean
whose meaning depends on which target variant accompanies it.

The 26.2 loader resolves tags into insertion-ordered sets, not repetition-preserving
lists. A function reached twice through direct or nested entries executes once at
its first resolved position. Stage 3 therefore:

- preserves entry order because first occurrence is semantic;
- rejects duplicate direct targets, regardless of `required` spelling;
- never models duplicate entries as repeated calls;
- tests nested overlap and external-tag composition against vanilla;
- treats the resolved contents of external tags as unknown for effects and cost.

All cycles among owned function tags are rejected, whether an edge is required or
optional. The 26.2 dependency sorter considers both classes and suppresses
cycle-forming edges, which can yield partial resolution rather than useful repeated
execution. Rejecting owned cycles gives the compiler one deterministic meaning.
External tag contents remain unknown and cannot be cycle-proven locally. Function
recursion remains legal and is analyzed separately from tag dependencies. Required
and optional cycle fixtures still pin the server behavior that motivates the stricter
compiler subset.

Duplicate serialized resource IDs within one resource kind and duplicate final
artifact paths are errors before emission. Equal IDs across the function and
function-tag kinds are legal.

## Closed initial command vocabulary

```text
CommandKind
  Score(ScoreCommand)
  Data(DataCommand)
  Execute(ExecuteCommand)
  Function(FunctionCall)
  Return(ReturnCommand)
  Raw(UnsafeRawCommand)
```

Normal builders create locally valid shapes. Recursive node representations and
their fields are not publicly constructible; callers use builders and read-only
views. This prevents bypassing structural-depth checks before verification. A
crate-private malformed test surface exists for verifier tests. There is no Stage 3
editor or optimization pass.

### Scoreboards

The initial set is the exact subset required to establish and manipulate compiler
state in a fresh world:

```text
ScoreCommand
  ObjectiveAddDummy { objective: ObjectiveName }
  PlayersSet { target: ScoreSelection, value: i32 }
  PlayersAdd { target: ScoreSelection, amount: NonNegativeI32 }
  PlayersRemove { target: ScoreSelection, amount: NonNegativeI32 }
  PlayersGet { score: ScoreRef }
  PlayersReset { target: ScoreSelection }
  PlayersOperation {
    target: ScoreSelection,
    op: ScoreOperation,
    source: ScoreSelection,
  }

ScoreSelection {
  holders: ScoreHolders,
  objective: ObjectiveName,
}

ScoreHolders
  Fake(FakeScoreHolder)
  Selector(Selector)
  AllTracked

SingleScoreHolder                  cardinality encoded by construction
  Fake(FakeScoreHolder)
  Selector(AtMostOneSelector)

ScoreRef                           exactly one score
  holder: SingleScoreHolder
  objective: ObjectiveName

ScoreOperation
  Assign | Add | Subtract | Multiply | Divide | Modulo | Min | Max | Swap
```

The command report confirms `players add/remove` accept nonnegative amounts and
`players operation` accepts multiple source and target holders. `PlayersGet` uses a
`ScoreRef` because vanilla requires a single holder. The safe type structure enforces
that restriction; the verifier repeats it only for crate-private malformed fixtures.
Multi-holder scoreboard commands are native bulk operations, not `execute` context
forks; their command contract reports no fork even though their effect category
covers multiple possible scores.

The 26.2 implementation applies `players operation` in a target-major nested loop:
for each selected target it applies the operation to each selected source in source
iteration order, mutating between pairs. This is one native command, but it is not a
simultaneous vector operation and is not generally equivalent to independently
pairing two holder sets. Stage 4 may select the multi-holder form only when those
sequential target semantics are intended; Stage 3 preserves the operand selections
exactly and performs no algebraic rewriting.

Objective-add is deliberately `dummy` only. Criteria, display names, objective
removal, and display configuration are not required by the compiler substrate.
Objective creation is an ordinary command chosen into a load function; the emitter
does not synthesize initialization or hide reload failure policy.

These variants mirror Minecraft behavior. They do not promise that one native score
operation realizes Core's integer semantics. Stage 4 must prove or synthesize the
chosen lowering for overflow, division, modulo, zero divisors, and multi-holder
sequencing rather than treating the shared operator names as semantic equivalence.

### Static selectors

The initial closed set encodes known cardinality rather than attaching an unchecked
field to a general selector:

```text
Selector
  AtMostOne(AtMostOneSelector)
  Unbounded(UnboundedSelector)

AtMostOneSelector
  Self
  NearestPlayer
  RandomPlayer

UnboundedSelector
  AllPlayers
  AllEntities

Cardinality
  AtMostOne
  Unbounded
```

`From<AtMostOneSelector>` and `From<UnboundedSelector>` construct the general
`Selector`; there is no fallible downcast from a general selector. Each selector
reports a conservative cardinality and context-read mask. Filters, entity kinds,
bounded counts, and user-facing selector types belong to Stage 7. Raw commands are
the temporary escape hatch.

### NBT and storage

Use a static, serializable SNBT subset:

```text
NbtValue
  Byte(i8) | Short(i16) | Int(i32) | Long(i64)
  Float(FiniteF32) | Double(FiniteF64)
  String(Box<str>)
  List(Vec<NbtValue>)
  Compound(Vec<(NbtKey, NbtValue)>)

NbtPath
  nonempty Vec<NbtPathSegment>

NbtPathSegment
  Key(NbtPathKey)
  Index(i32)
  AllElements

StoragePath {
  storage: StorageId,
  path: NbtPath,
}
```

Lists may be heterogeneous, as supported by current SNBT and `/data`. Compounds
reject duplicate keys and canonicalize keys lexicographically because compound
order is not semantic. Empty lists are valid. `NbtKey` permits an empty compound key;
`NbtPathKey` is a distinct nonempty wrapper because current `/data` traversal rejects
empty path keys. Conversion from `NbtPathKey` to `NbtKey` is infallible and the reverse
conversion is checked. SNBT escaping is a dedicated serializer and is not shared with
JSON escaping.

`NbtValue` is an opaque wrapper with builder constructors, not a publicly
constructible recursive enum. Construction and verification apply the same initial
depth limit as recursive commands. Recursive verification, dumping, and serialization
may use direct Rust recursion because they check the small depth bound before
descending. Crate-private malformed fixtures may exceed the limit by a small,
controlled amount to test diagnostics; the compiler does not need unbounded
adversarial-deserialization machinery for an IR it never deserializes.

Initial commands:

```text
DataCommand
  Get { source: StoragePath, scale: Option<FiniteF64> }
  Remove { target: StoragePath }
  Modify {
    target: StoragePath,
    mode: DataModifyMode,
    source: Value(NbtValue) | From(StoragePath),
  }

DataModifyMode
  Set | Merge | Append | Prepend
```

Root storage access, insert, arrays, string slicing, match filters, and block/entity
providers are deferred. Closed variants can add them without weakening this subset.

### Execute, context, and stores

```text
ExecuteCommand {
  modifiers: ExecuteModifiers,
  run: Box<CommandNode>,
}

ExecuteModifiers                         opaque, nonempty ordered sequence

ExecuteModifier {
  kind: ExecuteModifierKind,
  origin: OriginId,
}

ExecuteModifierKind
  As(Selector)
  At(Selector)
  In(DimensionId)
  If(Condition)
  Unless(Condition)
  Store(StoreChannel, StoreDestination)

Condition
  ScoreMatches(ScoreRef, ScoreRange)
  ScoreCompare(ScoreRef, ScoreComparison, ScoreRef)
  DataExists(StoragePath)
  EntityExists(Selector)

ScoreComparison
  Equal | LessThan | LessOrEqual | GreaterThan | GreaterOrEqual

StoreChannel
  Result
  Success

StoreDestination
  Score(ScoreRef)
  Storage {
    target: StoragePath,
    numeric_type: StorageNumericType,
    scale: FiniteF64,
  }

StorageNumericType
  Byte | Short | Int | Long | Float | Double
```

Modifier order is semantic and preserved. `Result` and `Success` remain distinct
through construction, dumping, tracing, and emission. They are Minecraft command
outcome channels, not implicit Core `i32`/`bool` values.

`ExecuteModifiers` is a small domain wrapper around a private
`Box<[ExecuteModifier]>`, not a third-party generic collection. The execute builder
requires a first modifier, collects any remaining modifiers in a temporary `Vec`,
and freezes them at finish. Read-only iteration exposes order without exposing an
empty constructor. This uses a checked construction boundary while keeping the final
representation ordinary Rust; it does not imply that every final vector must be
boxed before measurements justify it.

`ScoreComparison` mirrors the five comparison operators accepted by the target.
There is deliberately no `NotEqual` variant: inequality is expressed by `unless
score ... = ...`, preserving the actual command structure instead of inventing a
renderer-only operator.

Nested `execute ... run` is legal target syntax. Stage 3 does not flatten or reorder
it because doing so is an optimization with operation-count consequences. Recursive
command nodes have opaque construction and are acyclic through ownership. Builders
and verification enforce a small documented structural depth limit (initially 64)
to keep validation, rendering, dumping, and destruction comfortably bounded. Direct
recursive traversal is simpler and safe at that depth. This is a compiler safety
limit, not a claim about vanilla syntax; it can be raised with stress tests.

Position, rotation, anchors, facing, predicate conditions, and block/entity data are
added when a concrete lowering requires them.

### Functions and returns

```text
FunctionCall {
  target: CallableRef,
}

ReturnCommand
  Value(i32)
  Fail
  Run(Box<CommandNode>)
```

`return run` propagates both result and success in current Minecraft. It may wrap
any command shape the target parser accepts, including execute and return forms;
Stage 3 does not invent a semantic nesting ban. Structural depth still applies.

Calling a function tag runs its resolved insertion-ordered unique functions. An
ordinary tag call follows Minecraft's result-accumulation rules; under `return run`,
the first function return stops remaining tag execution. These are target semantics,
not permission for the emitter to expand a tag call into repeated function lines.

Function macros and `function ... with ...` are not represented by the typed call
API. Their argument ABI belongs to Stage 10. A raw command may deliberately escape
this restriction and therefore retains unknown semantics.

### Unsafe raw commands

```text
UnsafeRawCommand::new(line) -> Result<UnsafeRawCommand, RawCommandError>
```

The constructor accepts one physical command and rejects:

- empty input, CR, or LF;
- leading/trailing whitespace;
- a leading `/`, `#`, or `$`;
- a final `\`, which would continue onto the next emitted physical line;
- more than 2,000,000 UTF-16 code units.

It does not claim to parse Brigadier syntax. Raw commands have unknown effects,
context dependence, outcome behavior, and fork behavior. Their syntax authority is
the real server. Safe lowering code never creates them through incidental string
concatenation.

All structured commands are also checked against the same rendered UTF-16 length
limit. Rust byte length and Unicode scalar count are not substitutes for Java
`String.length()`.

## Conservative command contracts

Centralize only facts with an immediate consumer:

```text
CommandContract {
  effects: EffectSummary,
  context: ContextSummary,
  fork: ForkClass,
}

EffectSummary
  Known(EffectCategories)
  Unknown

EffectCategories
  ScoreRead | ScoreWrite
  StorageRead | StorageWrite
  EntityQuery
  Control

ContextSummary
  Known { reads: ContextMask, changes: ContextMask }
  Unknown

ForkClass
  Never | AtMostOne | Unbounded | Unknown
```

This is deliberately not alias analysis. Score/storage commands expose broad known
categories; calls and raw commands use unknown effects, context, and forks. Selector
cardinality contributes to fork classification only in a context-producing modifier
such as `as` or `at`. `if entity`/`unless entity` merely test whether a selection is
empty and never fork per matching entity. Execute derives its summary
conservatively from each modifier's role, its order, and the nested command; a
selector does not carry one context-independent fork classification of its own.

`ContextMask` names executor, position, rotation, dimension, and anchor. Execute
modifiers transform the nested command's context for that command line; there is no
mutable global context object. Context and fork summaries let Stage 4 reject obvious
scalar-context mistakes. They do not license Stage 3 reordering.

The initial modifier contracts are:

| Modifier | Context read | Context changed | Per-input fan-out |
| --- | --- | --- | --- |
| `as selector` | `selector.reads()` | executor | selector cardinality |
| `at selector` | `selector.reads()` | position, rotation, dimension | selector cardinality |
| `in dimension` | position, dimension | position, dimension | never |
| `if` / `unless` | condition-specific reads | none | never |
| `store` | destination-specific reads | none | never |

`in` reads the old position/dimension because changing dimensions may rescale
coordinates. A selector condition may read executor/position/dimension while
resolving its selection, but it only keeps or removes the incoming context. In
`ForkClass`, `AtMostOne` and `Unbounded` are always relative to each incoming
context. Store contributes its destination write effect even though that write is
performed from the nested command's outcome. Contract composition follows modifier
order and conservatively unions these facts; the summary is not an equivalence proof
and never permits modifier reordering.

Implement `EffectCategories` and `ContextMask` as small private-field integer
newtypes with named constants plus `union`, `contains`, and `intersects`; do not use a
fieldless enum or expose arbitrary raw bits. This is small enough not to justify
another dependency while still making set semantics explicit.

Place-level reads/writes, exact outcome algebra, transitive call summaries, and alias
analysis wait until Stage 5 has real transformations that need them.

## Stage 3 / Stage 4 legalization boundary

Stage 4 owns:

- block fusion, duplication, and outlining;
- branch policy selection;
- physical homes for SSA/block-argument values;
- generated function/objective/holder names;
- call/return conventions and tail-call choice;
- initialization policy and helper generation.

Stage 3 requires the chosen shape to be explicit:

1. an ordinary function call resumes at the next command;
2. `return run function` exits and propagates the called outcome;
3. one `execute if/unless` gates one nested command;
4. a multi-command branch is already a helper call or multiple separately justified
   command lines;
5. no Core branch, jump, block argument, or SSA value survives;
6. the emitter never invents helpers, scores, storage, or control flow.

This is full target legalization: emission is mechanical and cannot repair an
incomplete layout.

## Layered verification and checked emission

The public emission operation always verifies its input:

```text
emit_datapack(
  program: &MinecraftProgram,
  sources: &SourceContext,
  options: &EmissionOptions,
) -> Result<EmissionOutput, Diagnostics>
```

There is no public unchecked renderer. This is simpler than a borrowed typestate
witness, avoids threading a lifetime-only wrapper through the pipeline, and matches
the compile APIs of mature Rust backends: validation is an internal mandatory phase
of producing output. Verifier layers remain separately callable inside the crate for
focused tests.

Structural verification accumulates safe independent diagnostics in this order:

1. target identity;
2. ID range and dense-store ownership;
3. resource/objective/holder/range/NBT/numeric validity;
4. same-kind duplicate serialized resources and reserved artifact paths;
5. local command shape, score cardinality, execute ordering, and structural depth;
6. internal function/tag reference integrity;
7. internal tag dependencies, direct duplicates, load/tick replacement policy, and
   all owned tag cycles;
8. raw-line physical-line rules;
9. origin validity and output-path uniqueness.

The target renderer is not called until all structural layers succeed. Diagnostics
from those layers use IDs, validated leaf views when available, and the generic
malformed-safe dumper; they never invoke target `Display`/rendering on a node whose
shape has not been established. Later layers rely only on invariants proven above
them instead of defensively rechecking everything.

Exact logical-line length is intentionally not a structural verifier pass: computing
it separately would duplicate target serialization. After structural success, the
only expected command-rendering rejection in the safe modeled subset is the explicit
2,000,000-UTF-16-unit limit detected by `CommandSink`. Any other recoverable syntax
failure indicates a missing construction/verifier invariant and must gain a focused
test rather than becoming a general renderer error path.

All lookups remain fallible even when an earlier layer should have proven them safe,
so a verifier bug becomes another diagnostic rather than a panic. Malformed IR
must never panic diagnostics or dumping. Function recursion is legal. Heterogeneous
NBT lists are legal. Empty function bodies are legal. Tag-cycle and dependency
analysis is iterative so a long valid acyclic tag chain cannot overflow the host
stack.

Diagnostics are emitted by stable program/function/command traversal, never by map
iteration. Verifier symbol tables may use `HashMap` for lookup speed because their
iteration order is unobservable; the first-seen ID and every reported collision are
chosen from the surrounding dense-store order. Graph worklists explicitly preserve
declared tag order when several nodes become ready at once.

A single deterministic `MinecraftDebugDumper` accepts valid or malformed IR,
includes allocated identities, invalid references, nested origins, and explicit raw
barriers, and is used in failure reports. Canonical target text is the emitted
artifact itself; Stage 3 does not maintain a second verified pseudo-syntax with
duplicate formatting rules.

## Deterministic in-memory emission

```text
EmissionOutput {
  pack: DatapackArtifact,
  trace: TraceMap,
}

EmissionOptions {
  description: Box<str>,
}

DatapackArtifact {
  files: Vec<PackFile>,                 // sorted by PackPath
}

PackFile {
  path: PackPath,
  bytes: Vec<u8>,
}
```

The initial options contain only the pack description; there is no debug-comment,
layout, optimization, or compatibility switch hidden in emission. After structural
verification succeeds, the emitter renders every command exactly once into its final
function buffer while simultaneously counting Java UTF-16 code units and producing
trace records. It performs no semantic repair. Any render or length diagnostics
discard the incomplete artifact. The artifact contains only:

```text
pack.mcmeta
data/<namespace>/function/<path>.mcfunction
data/<namespace>/tags/function/<path>.json
```

The trace map is a compiler-side artifact, not an undeclared datapack file.

Determinism rules:

- sort files lexicographically by normalized `/` path;
- preserve command and function-tag entry order;
- canonicalize only semantically unordered compound keys;
- serialize fixed private JSON DTO structs in declaration order;
- emit UTF-8, LF, and one terminal newline for nonempty text files;
- emit an empty function as zero bytes;
- exclude timestamps, absolute paths, random salts, debug hashes, and hash-map order;
- reject duplicate paths rather than choosing a winner;
- produce byte-identical output for identical valid input and options.

Use `serde` derives and `serde_json::to_vec` only on private pack-metadata and
function-tag output DTOs. Do not derive serialization for compiler IR. Structs and
ordered vectors make field and entry order explicit while the library owns correct
JSON escaping. Append the required LF after successful serialization. SNBT remains a
separate target serializer; JSON and SNBT escaping are never shared.

The command renderer writes through a small `CommandSink` that owns the final
`Vec<u8>` and a UTF-16-unit counter. `push_str` checks the added UTF-16 length before
appending the already-rendered UTF-8 slice and refuses to grow beyond the target
limit. This keeps command construction to one rendering pass, avoids a temporary
`String`, and prevents confusing byte-length checks for non-BMP Unicode.

Create one sink per function. After one top-level command renders successfully,
`finish_command` appends LF, resets the logical-line counter, and appends that
command's origin to the function trace. LF is not part of the Minecraft logical-line
limit. On any failure the entire sink and partial trace are discarded; commands are
never copied through a per-line staging `String` or `Vec`.

Filesystem writing is outside `emit`. The harness installs artifact entries below a
new sandbox pack directory after defensively revalidating `PackPath`. A reusable
public writer waits until another caller exists.

## Traceability

```text
TraceMap {
  target: JavaEditionTarget,
  functions: EntityVec<McFunctionId, FunctionTrace>,
}

TraceMap::pack_format() -> [u32; 2]      // derived from target

FunctionTrace {
  function: FunctionResourceId,
  origins: EntityVec<CommandId, OriginId>,
}

TraceRecord<'a> {                        // iterator view, not stored
  function_id: McFunctionId,
  function: &'a FunctionResourceId,
  command: CommandId,                   // derived from origins index
  origin: OriginId,
}

TraceRecord::line() -> NonZeroU64        // u64(command.index()) + 1
```

Every nonempty emitted top-level command produces exactly one trace record using the
top `CommandNode` origin. Storage groups origins by function so the function resource
is retained once rather than cloned into every record. `TraceMap::records` yields the
flat views when a consumer wants them. Physical line is derived because dense command
order and physical line order are identical; it cannot disagree with `CommandId`, and
widening before addition avoids the final-`u32` edge. Empty functions have a
`FunctionTrace` with an empty origin table and yield no records. Nested node and
modifier origins remain available in the IR dump and diagnostics but do not invent
extra physical lines.
Call-site and fused origins remain references into `SourceContext`; they are not
flattened to a misleading span. A direct Stage 3 caller must retain the same
`SourceContext` passed to `emit_datapack`; the future top-level compilation result
will own that context alongside the artifact and trace so IDs cannot be accidentally
resolved against another compilation.

Initial output has no provenance-comment mode. If debug comments are added later,
they require a separately verified option and their own line-accurate trace. A
terminal raw continuation is forbidden specifically so one `CommandId` cannot
consume another command's physical line.

Emission failures report the target and, when applicable, resource ID, command ID,
origin, malformed-safe dump, and planned artifact paths. They do not expose or claim
the incomplete artifact is runnable.

## Vanilla integration boundary

Add a generic harness method:

```text
ServerSandbox::install_datapack(pack_name, artifact.files())
```

The outer pack name and every relative artifact path are validated before writing.
The harness method accepts generic validated relative path/byte entries, not a
compiler-specific artifact type. `mdl-test` may use `mdl-compiler` as a dev-dependency
for the ignored integration test; its ordinary harness library stays independent of
the compiler, and `mdl-compiler` never depends on `mdl-test`.

The direct-Minecraft-IR test:

1. constructs IR without Core or a parser;
2. emits twice and compares all paths/bytes;
3. installs before startup;
4. creates a dummy objective from `minecraft:load` in a fresh world;
5. invokes an exported-by-resource-ID test function;
6. covers score set/add/remove/get/operation and score conditions;
7. covers storage value/from/get and separate result/success destinations;
8. covers execute context, internal function and tag calls, return value/fail/run;
9. executes one unmistakable raw marker;
10. validates trace lines against emitted files;
11. fails on attributable pack-load/function-parse warnings or errors;
12. preserves pack, trace, world, and logs on failure.

The test also pins edge fixtures for empty functions, tag dependency cycles, raw
continuation rejection, unusual accepted/rejected names, and the UTF-16 line limit.
Boundary fixtures may use smaller generated cases when allocating a two-million-unit
line would make the fast suite unreasonable; the actual target boundary remains an
ignored conformance test.

The harness needs a log checkpoint so readiness cannot hide earlier datapack parse
errors. A client is not required. Fast tests remain server-free.

## Implementation gates

Every gate leaves formatting, Clippy, fast tests, and rustdoc clean.

### Stage 3A: Shared foundation, target facts, and validated atoms

First extract the existing diagnostics into `crate::diagnostic` as a
behavior-preserving change with Core re-exports and tests unchanged. Then implement
the closed 26.2 target, identifiers, objective/holder/range types, finite numbers,
selector cardinality, static NBT values/paths, and resource-to-pack-path mapping.
Do not add `serde` yet; its first consumer is Stage 3D.

Exit criteria:

- target facts match official reports and binary conformance fixtures;
- Core uses the shared crate-level diagnostic containers without changing its public
  re-exports or diagnostic ordering;
- unsafe/traversing pack paths cannot be constructed;
- score ranges and finite values round-trip canonical text;
- SNBT escaping covers control characters, quotes, slashes, Unicode, and numeric
  suffixes;
- no constructor performs I/O or allocates generated compiler names.

### Stage 3B: Program, command vocabulary, and contracts

Implement functions, tags, typed IDs, builders, command variants, conservative
contracts, depth checking, and the deterministic malformed-safe dumper.

Exit criteria:

- objective creation makes typed score programs runnable in a fresh world;
- two-phase declaration supports mutually recursive functions and forward tag
  references while rejecting a second definition;
- builder finish rejects missing definitions and final IR contains no optional
  definition state;
- function and tag calls are distinct and internal references are ID-based;
- external-by-name references cannot alias owned resources;
- nested command/modifier and tag-entry failures retain their own origins;
- score comparisons cover exactly the target's five native operators;
- result and success cannot be interchanged;
- an execute command cannot be finalized without a modifier;
- order is preserved for commands, execute modifiers, and tag entries;
- calls/raw commands are unknown barriers;
- main node sizes are recorded before considering packed storage;
- macros and continued physical lines are absent from safe structured construction.

### Stage 3C: Verifier and command renderer

Implement all structural verifier layers and the single target command renderer used
by checked emission.

Exit criteria:

- each verifier rule has a focused negative test;
- malformed raw fixtures accumulate diagnostics without panic;
- function recursion and heterogeneous lists verify;
- owned tag cycles, invalid cardinality, broken references, and path collisions
  are rejected;
- empty functions verify;
- rendered logical lines obey the UTF-16 limit and never continue physically;
- rendering writes final bytes and counts UTF-16 units in one pass;
- no unchecked renderer is public;
- after structural success, rendered-length exhaustion is the only expected
  command-rendering diagnostic in the safe modeled subset.

### Stage 3D: Artifact and trace emitter

Implement the checked public emission entry point, private serde JSON DTOs, exact
metadata, function/tag files, sorted artifact output, and trace maps.

Exit criteria:

- golden snapshots assert exact paths and bytes;
- JSON escaping is delegated to `serde_json` without making IR serializable;
- repeated emission is byte-identical;
- every emitted command has one correct trace record;
- empty functions have zero bytes and zero trace records;
- unordered compounds canonicalize while ordered target lists do not;
- duplicate or traversing paths cannot reach artifact output.
- a 100,000-command server-free case checks counts and bytes without whole-program
  cloning, and a separate geometric benchmark records scaling without a CI timing
  threshold.

### Stage 3E: Vanilla conformance

Extend the harness minimally and run the direct-IR pack on the pinned 26.2 server.

Exit criteria:

- the pack loads with no attributable parse error;
- every modeled command family executes;
- result and success are observed independently;
- objective initialization, function tags, empty functions, and return shapes match
  recorded expectations;
- reports identify the exact generated command on failure;
- ordinary workspace tests start no server.

### Stage 3F: Stage 4 handoff audit

Hand-construct all three planned branch shapes plus ordinary and tail calls. Record
the exact target operations or context modifiers Stage 4 still lacks.

Exit criteria:

- Stage 4 needs a lowering/layout/physical-allocation pass, not emitter redesign;
- no Core value or block representation is baked into target APIs;
- the emitter never creates helpers or chooses a branch/call policy;
- speculative editors, schedulers, macro ABIs, and selector frameworks remain absent.

## Required proof corpus

At minimum, automate:

1. target facts and exact `pack.mcmeta`;
2. valid/invalid resource IDs, legal equal function/tag IDs, and artifact traversal
   cases;
3. objective, fake-holder, selector, and score-range boundaries;
4. every NBT scalar, heterogeneous lists, compounds, paths, escaping, and depth;
5. dummy objective creation and every initial player-score operation;
6. scalar versus multi-holder legality and target-major sequential operation
   semantics;
7. storage get/remove/modify rendering and static-path validation;
8. execute modifier order, condition inversion, context masks, and fork classes;
9. all five native score comparisons, execute non-emptiness, and distinct
   result/success stores;
10. function calls, tag calls, mutual recursion, forward references, missing or
    repeated definitions, broken internal references, and rejection of same-kind
    external references aliasing owned resources;
11. return value/fail/run and nested execute composition;
12. ordered/optional/nested function tags, direct duplicate rejection, nested
    overlap de-duplication, long acyclic chains, load/tick policy, and owned-cycle
    rejection;
13. empty functions and empty-tag call behavior;
14. raw rejection for newline, comment/macro/slash prefix, terminal `\`, whitespace,
    and UTF-16 length;
15. unknown call/raw effects and no Stage 3 reordering;
16. command/NBT structural-depth rejection with a small controlled over-limit
    fixture;
17. invalid origins and malformed IDs without verifier/dumper panic;
18. checked public emission, exact function/tag bytes, and deterministic file
    ordering;
19. trace correspondence for top-level versus nested origins, including empty
    functions;
20. one real vanilla execution covering the complete initial vocabulary;
21. one large server-free construction/emission proof plus a non-gating geometric
    benchmark for quadratic behavior and accidental whole-program cloning.

## Explicit deferrals

Stage 3 does not implement:

- Core-to-Minecraft lowering, branch selection, physical allocation, or generated
  name allocation;
- user-facing syntax, HIR, typed command APIs, or raw interpolation;
- function macros or a macro frame ABI;
- selector filters, entity kinds, source-level context capabilities, or numeric
  cardinality proofs;
- root storage operations, block/entity NBT, filtered/dynamic paths, arrays, insert,
  or string slicing;
- rich text, positions, rotations, anchors, facing, predicates, items, or world
  mutation commands;
- place-level alias effects, exact outcome algebra, transitive function summaries,
  command reordering, or cost models;
- command-sequence proofs, scheduling, yielding, or runtime queues;
- multiple targets, pack overlays, zip packaging, incremental output, or a reusable
  general filesystem writer;
- external datapack ABI guarantees or cross-pack objective collision prevention;
- an embedded Brigadier parser, e-graphs, profiling, or benchmark weights.

Later stages extend closed enums only when a concrete lowering or typed language
feature demands it.

## Completion commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Target conformance:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test minecraft_ir -- --ignored --nocapture
```

Stage 3 is complete only when the ignored real-server test passes.

## Primary sources

Compiler architecture:

- [Zig ZIR: immutable source-independent IR consumed by semantic analysis](https://github.com/ziglang/zig/blob/master/lib/std/zig/Zir.zig)
- [Zig AIR: analyzed per-function IR consumed by code generation](https://github.com/ziglang/zig/blob/master/src/Air.zig)
- [Zig backend-specific AIR legalization](https://github.com/ziglang/zig/blob/master/src/Air/Legalize.zig)
- [Current Zig backend MIR direction](https://ziglang.org/devlog/2026/)
- [GHC's parameterized STG passes and final STG-to-Cmm boundary](https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/src/GHC.Stg.Syntax.html)
- [GHC IR dumps, determinism controls, and per-level linting](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/debugging.html)
- [OCaml Lambda algebraic IR](https://github.com/ocaml/ocaml/blob/trunk/lambda/lambda.mli)
- [OCaml machine-independent Cmm IR](https://github.com/ocaml/ocaml/blob/trunk/asmcomp/cmm.mli)
- [Cranelift dense typed entity maps](https://docs.rs/cranelift-entity/latest/cranelift_entity/struct.PrimaryMap.html)
- [Cranelift temporary `FunctionBuilder` and consuming `finalize`](https://docs.rs/cranelift-frontend/latest/cranelift_frontend/struct.FunctionBuilder.html#method.finalize)
- [Cranelift IR verifier responsibilities](https://docs.rs/cranelift-codegen/latest/cranelift_codegen/verifier/)
- [Cranelift checked `Context::compile` entry point](https://docs.rs/cranelift-codegen/latest/cranelift_codegen/struct.Context.html#method.compile)
- [LLVM MachineInstr-to-MC emission boundary](https://llvm.org/docs/CodeGenerator.html#code-emission)
- [Rust API guidance on newtypes and custom argument types](https://rust-lang.github.io/api-guidelines/type-safety.html)
- [Rust API guidance on validating construction boundaries](https://rust-lang.github.io/api-guidelines/dependability.html)
- [rustc typed `IndexVec`](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_index/vec/struct.IndexVec.html)
- [rustc MIR `Body.phase` and why mixed optimization progress is recorded](https://doc.rust-lang.org/beta/nightly-rustc/rustc_middle/mir/struct.Body.html#structfield.phase)
- [rustc MIR statements carry source information](https://doc.rust-lang.org/beta/nightly-rustc/rustc_middle/mir/struct.Statement.html)
- [Swift raw versus canonical SIL stages](https://github.com/swiftlang/swift/blob/main/docs/SIL/SIL.md#sil-stages)
- [`serde_json::to_vec` output serialization](https://docs.rs/serde_json/latest/serde_json/fn.to_vec.html)
- [MLIR full target legalization](https://mlir.llvm.org/docs/DialectConversion/)
- [MLIR operation builders, constraints, and verification ordering](https://mlir.llvm.org/docs/DefiningDialects/Operations/)
- [The Nanopass compiler methodology](https://www.cambridge.org/core/journals/journal-of-functional-programming/article/educational-pearl-a-nanopass-framework-for-compiler-education/1E378B9B451270AF6A155FA0C21C04A3)

Minecraft target behavior:

- [Minecraft Java 26.2 release and data-pack format 107.1](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
- [Minor pack versions and current `min_format` / `max_format` metadata](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-9)
- [Function outcomes, return propagation, and command limits](https://feedback.minecraft.net/hc/en-us/articles/21968446892173-Minecraft-Java-Edition-1-20-3)
- [The distinct `execute store result` and `execute store success` channels](https://www.minecraft.net/en-us/article/minecraft-snapshot-17w45a)
- [Function macro lines and physical line continuation](https://feedback.minecraft.net/hc/en-us/articles/18619031671821-Minecraft-Java-Edition-Snapshot-23w31a)
- [Heterogeneous SNBT lists and empty-path-key behavior](https://feedback.minecraft.net/hc/en-us/articles/35298208390797-Minecraft-Java-Edition-1-21-5-Spring-to-Life)
- [Official Java dedicated-server download](https://www.minecraft.net/en-us/download/server)
- [Generated datapack-structure reports](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-2)
