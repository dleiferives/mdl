# Stage 7: Typed Minecraft Programming Model

Status: **complete and gated**

The codebase-audited dependency order used to implement the stage is retained in
[`stage-7-remainder-plan.md`](stage-7-remainder-plan.md). This document remains the
broader language-design rationale; the resulting contract is summarized in
[`stage-7-handoff.md`](stage-7-handoff.md).

Stage 7 is the first stage that defines what writing Minecraft behavior in MDL feels
like. It does not attempt broad command coverage. It establishes the semantic seams
that let coverage, target versions, representations, scheduling, and optimization
grow without turning command strings into the language's internal model.

The stage is split into four independently gated tranches:

| Tranche | Outcome |
| --- | --- |
| 7A | Zig-like whole-package module graph, namespaces, visibility, and exports |
| 7B | Conservative external-effect Core substrate and literal unsafe command |
| 7C | Minecraft semantic types, entity cardinality, execution context, and function behavior summaries |
| 7D | Method-first typed command API, central semantic registry, and version-specific lowering recipes |

This supersedes conflicting details in the Stage 6 handoff plans. In particular,
modules are no longer a flat globally named bag, context changes are not flat mutable
instructions, and typed commands are not implemented in the same tranche as the Core
external-operation substrate.

## Design invariants

Stage 7 preserves these separations:

1. A module namespace is a compile-time value; an entity or selector is a runtime
   semantic value.
2. A single entity reference, a possibly-many entity query, and the current executor
   are different types. Cardinality is never guessed from surface spelling.
3. An operation's source meaning is target-independent. A selected target recipe
   decides whether and how that meaning can be emitted.
4. Ambient context requirements, scoped context changes, entity forks, world effects,
   observable output, and command outcome are independent facts.
5. Semantic behavior and physical cost are independent. One source operation may
   lower to zero, one, or many commands and helpers.
6. Compiler-owned typed operations and unsafe raw commands are deliberately unequal.
   Unsafe text never acquires optimizer privileges by looking familiar.
7. Context does not leak from one expression to the next. Any change is represented
   by an operation receiver or a structured lexical region.
8. A source construct that can multiply execution makes that multiplicity visible.

These are more important than the first surface syntax. Syntax can be refined after
the types and behavior are stable.

## Stage 7A: package graph and Zig-like namespaces

### Driver-owned graph

The compiler library receives a complete, owned compilation graph. It never resolves
an import by opening a path:

```text
PackageInput {
  root: ModuleKey
  modules: [ModuleInput]
}

ModuleInput {
  key: ModuleKey
  source: SourceInput
  dependencies: [ModuleDependency]
}

ModuleDependency {
  name: ImportName
  target: ModuleKey
}
```

`ModuleKey` is a driver-owned unique identity. It is not a filename and need not be
source-visible. `SourceInput::name()` remains diagnostic metadata. Each module has a
local dependency map from a source-visible name to another `ModuleKey`, matching
Zig's useful distinction between compilation-module identity and the names through
which one module sees dependencies.

This replaces the former global `ModulePath` namespace. A module cannot name an
arbitrary package member merely because it knows a canonical string. The driver or a
future manifest constructs the graph and chooses local dependency names first.

Only modules reachable from `PackageInput::root` participate in a valid compilation.
Supplying an unreachable module is rejected as input drift rather than silently
changing diagnostics or output. Dependency cycles are accepted because Stage 7 has
no module initialization or top-level mutable execution. All reachable function
signatures are collected before bodies are checked.

This is intentionally a whole-package rule, not yet a promise about independently
compiled interfaces. Zig's compilation model explicitly permits dependency loops.
GHC's `hs-boot`/`SOURCE` mechanism shows why the rule must be revisited if MDL later
adds separate compilation, inferred exported types, or top-level initialization:
those features may require an explicit cycle-breaking signature contract rather than
the current all-signatures-first package index.

The library normalizes by `ModuleKey`, dependency name, and source order before
allocating compilation-local IDs. Caller vector order, filesystem traversal order,
and absolute paths never affect emitted bytes.

Frontend limits distinguish per-module token/depth bounds from whole-package module,
token, and retained-diagnostic budgets. Whole-package tokens include each module's
EOF token. Exhaustion is reported in canonical module order and never causes the
compiler to continue allocating an unbounded `modules × per-file-limit` inventory.

### Namespace values and member access

An imported module is a compile-time namespace value. The intended source model is
Zig-like dotted access:

```text
const cells = import("cells");

export fn run(input: Int32) -> Int32 {
    return cells.normalize(input);
}
```

The exact declaration spelling remains a surface decision, but the AST should use
one postfix expression spine:

```text
primary (member_access | call)*
```

Resolution then determines whether `cells.normalize` is a namespace member,
`player.say` is a method, or a later expression is a field. The parser must not bake
module calls into a separate `alias::function` grammar.

Namespace values exist only while resolving source. They are not runtime HIR values,
Core operands, or Minecraft representations. A resolved call contains a stable
`SourceFunctionId`, not its import spelling.

### Visibility, root, and exports

Functions retain three meanings:

| Form | Same module | Importing module | Datapack entry |
| --- | --- | --- | --- |
| `fn` | yes | no | no |
| `pub fn` | yes | yes | no |
| `export fn` | yes | yes | yes |

`export` implies public visibility. Exported functions in any reachable module are
datapack entries for this tranche. The distinguished root controls graph membership,
not export eligibility. A later package/ABI stage may restrict or re-export a stable
public surface.

HIR retains source visibility, while Core retains only target-entry linkage:
`private` and `pub` both become `Internal`; `export` becomes `DatapackExport`.
This follows the separation between symbol visibility and linkage made explicit by
MLIR's LLVM dialect, and the declaration-level linkage model used by Cranelift. The
lowering report and ABI correlation retain all functions, but only
`DatapackExport` entries are published as external cost-analysis roots.

Stage 7 does not promise a stable emitted resource name. The compilation report maps
each source export to its generated resource. Stable cross-datapack ABI names remain
Stage 12.

An export may provisionally require ambient Minecraft context, including a typed
executor. Its inferred `FunctionBehavior` is part of the exported-entry report.
MDL callers must satisfy the requirement statically; an external datapack caller is
responsible for invoking the generated function under an equivalent `execute`
context. Stage 7 does not force every useful Minecraft entry point to erase its
native context merely to be exportable.

### Stage 7A boundary

Stage 7A adds no package manager, filesystem convention, network resolution,
serialized interface, incremental compilation, top-level initialization, import
glob, or re-export system. The current single-source API becomes a root-only adapter
through the same package pipeline.

## Stage 7B: external effects and unsafe raw commands

### Cross-compiler constraints on the substrate

The external-operation boundary is closed even though MLIR supports dynamically
extensible dialects. MDL is one compiler with one Minecraft semantic registry, so a
Rust enum gives exhaustive verification and pass auditing that an open operation map
would give up. MLIR's ODS motivation identifies the failure mode directly: generic,
string-keyed operations tend toward repetitive string comparisons and incomplete or
duplicated verification. Runtime-extensible operations solve a different product
problem.

The program-owned declaration plus instruction reference follows the useful part of
Cranelift's function preamble: a compact instruction reference resolves through an
inventory that owns the signature and external identity. It also follows MLIR's
separation between an operation's SSA operands and its auxiliary symbol references.
Unlike a linker symbol, however, an `ExternalOpId` is build-local semantic identity;
it is never a source name or stable datapack ABI.

Every non-SSA function edge must be discoverable through one Core query. MLIR uses
`CallOpInterface`/`SymbolUserOpInterface` so call graphs, verification, and symbol
rewrites do not each rediscover special operation layouts. Rust MIR similarly uses
generated visitors to centralize structural traversal. MDL needs the narrower closed
equivalent: one function-reference enumerator used everywhere, with tests that add a
call-like external binding and fail if any consumer omits it.

Raw commands receive no source syntax for claiming purity, memory/world effects,
termination, or clobbers. Rust and Zig inline assembly allow optimizer privileges
only through explicit contracts whose violation is unsafe or illegal behavior; GHC
primops likewise separate failure and side-effect facts because they control
floating and elimination. MDL cannot verify equivalent claims about arbitrary
Brigadier text, so its literal escape is always an observable, non-speculatable,
opaque black box. This deliberately sacrifices possible optimization at the unsafe
site rather than letting one incorrect annotation invalidate surrounding typed code.

GHC often orders effects with explicit state-token dependencies. MDL does not add a
fake world-token SSA value in Stage 7B: Core already gives attached unknown-effect
instructions observable block order, and adding a token would contaminate every
typed Minecraft signature without improving the current optimizer contract. If a
future scheduler needs an explicit effect dependency graph, it must be derived from
verified behavior summaries rather than exposed as a forgeable source value.

### Narrow Core substrate

Minecraft effects enter Core through a closed linked declaration, not target text:

```text
ExternalOpId
TargetFragmentId

CoreOp::External(ExternalOpId)

ExternalOpDecl {
  semantic_binding
  parameter_types
  result_types
  attributes
  origin
}

ExternalSemanticBinding =
    MinecraftOperation(MinecraftOperationId)
  | MinecraftRunScope(RunScopeId)
  | UnsafeTargetFragment(TargetFragmentId)
```

7B lands the raw case and the closed reference-enumeration hook; 7C adds the
run-scope inventory and binding as its first concrete call-like consumer; 7D adds the
first program-owned typed Minecraft-operation inventory.

The declaration and fragment inventories live inside `CoreProgram`. Runtime values
are ordered typed SSA operands. Static attributes use a closed Rust enum; there is no
string-keyed attribute bag, dynamic operation plug-in, or user-defined external
effect.

Initially every external operation has the conservative Core contract already used
for ordinary calls:

```text
effects()            = Unknown
speculation()        = Never
result_equivalence() = Opaque
```

This prevents DCE, CSE, motion, speculation, or duplication without a later explicit
proof. Rich Minecraft contracts remain available to lowering, cost analysis, and the
Stage 9 scheduler; Core optimization does not need a premature alias/effect lattice.

### HIR does not copy registry truth

A typed HIR external node stores only its resolved semantic key, typed operands,
closed attributes, result types, and origin. It does not copy context/effect/fork
descriptors from the compiler registry. Verification looks up the authoritative
descriptor and recomputes consistency. This prevents cached semantic data from
drifting when a command descriptor changes.

### Structured run scopes are call-like external operations

A multi-statement contextual block cannot be lowered by applying its modifier chain
to each command independently. Under a many-entity `as`, that would change:

```text
for each entity: A; B
```

into:

```text
for every entity: A
for every entity: B
```

Those programs can observe different world states. HIR therefore retains one
`HirRun` region. Core generation outlines the body into one internal function
and emits one external, call-like invocation:

```text
RunScopeId

RunScopeDecl {
  modifiers: [RunModifierInstance]
  callee: FunctionId
  invocation_bounds
  origin
}

RunModifierInstance =
    AsEntityQuery { query: EntityQueryId, origin }
  | ... closed typed modifier variants
```

The first slice uses a closed typed enum because execute overloads have distinct
context, fork, and outcome behavior; a generic semantic-key/attribute record would
make those invariants easier to omit. Future runtime modifier inputs and ordinary
lexical captures remain explicit ordered operands of the enclosing external
invocation, with each typed modifier referring to its corresponding operand range.
The executor capture is a scoped context capability, not a hidden UUID or score
value. The first scope is `Void`; no value escapes a zero/many invocation.

The final body-invocation bound is not the complete target fork proof. Minecraft
checks ordinary redirect expansion at each execute-chain prefix, using a strict
boundary against `minecraft:max_command_forks`; custom paths such as
`if function` do not use that same guard. Target legality must therefore derive an
ordered prefix ledger containing both the context bound and the redirect-guard class
of every modifier. It must also publish the minimum server gamerule assumption under
which the emitted chain is admitted. A Stage 7 proof that every prefix is at most one
is an ABI support proof, not a claim that the chain runs when the server configures
fork limit zero or one.

Semantic query data remains origin-free and reusable. HIR and Core declarations wrap
it with occurrence provenance for the root and each refinement step, so an invalid
target limit or future unsupported filter is diagnosed at the exact method that
introduced it rather than at the enclosing run block.

The outlined `callee` is a real Core function reference. One closed Core helper
enumerates zero or more function references from ordinary calls and call-like
external bindings. Target semantic inventory uses that helper, and its downstream
reachability, call-graph/SCC, recursion, and lowering analyses consume the resulting
edges. Core declaration verification checks referential shape structurally; frontend
behavior inference operates on HIR; ID-stable cloning retains declarations without
rediscovering edges. The plural enumeration contract is required before modifiers
such as `if function` add an edge in addition to the final outlined-body edge.

The contextual invocation remains `Unknown/Never/Opaque`. Baseline lowering emits
one ordered `execute ... run function <outlined>` operation after materializing
ordinary captures. A later proven recipe may place a compatible one-command body
directly under `run`; it may not duplicate the modifier chain or change per-context
statement order merely to save a helper.

### Literal unsafe boundary

The first escape hatch is visibly unsafe, target-specific, statement-only, and
compile-time literal-only:

```text
unsafe minecraft("say hello");
```

It returns no value and has unknown effects, context, outcome, transitive work, and
fork behavior. Target-independent validation rejects invalid physical-line shape;
the selected target later checks encoding and length. The compiler does not parse the
line into safe semantics or infer behavior from its text. A line that passes compiler
shape checks may still be rejected by the real server, which is part of the explicit
unsafe contract.

Runtime interpolation and Minecraft function macros remain Stage 10. Users cannot
assert that raw text is pure, non-forking, or portable.

### Why 7B is separate

Stage 7B can be proven before entity types and command families exist. Its first
vertical proof is a literal unsafe line moving through source, HIR, Core,
optimization, structured target ownership, emission, and the pinned server. This
keeps the hardest closed-IR audit independent of evolving Minecraft API design.

### Implemented 7B gate

The implemented slice keeps target-independent line shape in one shared validator,
then performs Java UTF-16 length validation only after selecting
`JavaEditionTarget::V26_2`. Source/HIR/Core inventories retain stable identities and
origins; the physical plan has a distinct external instruction and dedicated
one-line isolation helper rather than pretending raw text is scalar work. The helper
is semantically required: an opaque command may contain `return`, and emitting it
directly into compiler block functions would make its control effect depend on block
fusion. A normal call to the isolation helper contains `return` to that helper and
preserves source-statement continuation. All Core optimizers retain the operation as
an ordered `Unknown/Never/Opaque` barrier.

The vertical differential runs all four Core/Minecraft `None|Baseline` policy
combinations. The official Mojang 26.2 bundler server loads and executes the emitted
literal command without a client. A separate server fixture proves the boundary by
compiling an unknown Brigadier command successfully and then observing vanilla reject
the resulting function during datapack loading. Compiler-owned physical hazards and
target-length overflow are separate deterministic diagnostics.

## Stage 7C: semantic values, context, and multiplicity

### Entity categories

The source type system starts with semantically distinct categories rather than a
single stringly selector:

```text
EntityRef<T>       // exactly one entity of semantic kind/refinement T
EntityQuery<T, C>  // a selection with kind/refinement T and cardinality C
Executor<T>        // a bound current executor within a lexical context
```

`Player`, `Zombie`, and `ArmorStand` are nominal semantic entity kinds. Each kind has
an inferred closed capability set; a kind is not a promise that an arbitrary selector
matches exactly one entity. The initial cardinality classes are closed and conservative:

```text
ExactlyOne
AtMostOne
Bounded(n)
Unbounded
```

Stage 7 represents these few canonical semantic types directly as small closed Rust
keys. It does not add a global type interner yet. Rust and Zig need interners for
large recursive, generic type universes; this slice has one nominal entity kind and
shallow parameterized keys, so structural equality is already canonical and cheap.
An interner becomes justified when a later aggregate/type-system stage introduces
recursive structural types or measured duplication, not merely because a larger
compiler uses one. Stage 8 is deliberately scalar and does not meet that threshold.
This keeps the semantic layer closer to Cranelift's compact closed value types while
retaining an obvious migration path to rustc/Zig-style interned identities.

Kind/refinement, cardinality, and runtime representation are deliberately separate
axes. GHC's nominal/representational role distinction is a useful warning here: two
entity kinds may eventually share a target representation without becoming the same
semantic type. Likewise, GHC's multiplicity types describe use of a value, not the
number of Minecraft execution contexts. MDL therefore keeps query cardinality and
run-scope invocation bounds in their own closed domains instead of overloading value
linearity or Core storage types.

Conversions that weaken knowledge are implicit only when harmless. Strengthening
from a query to `EntityRef` requires an operation whose absence/multiplicity behavior
is explicit. A query builder produces an `EntityQuery`, never an
`EntityRef` solely because it commonly matches one entity.

Methods on `EntityRef` never hide a fork. Operations on a possibly-many query require
explicit iteration/fork syntax such as `run.as(query)`. There is no implicit
auto-vectorization of scalar entity methods.

### Queries and semantic capabilities

The provisional source API uses a fluent, typed query builder rather than exposing
raw selector strings or adding a separate query-comprehension grammar:

```text
mc.entities(ArmorStand)
    .with_tag("prisoner")
    .limit(1)
```

This is a compiler-owned closed query plan, not a runtime list or arbitrary user
closure. The kind parameter and cardinality evolve through construction:

```text
mc.entities(ArmorStand)   : EntityQuery<ArmorStand, Unbounded>
query.limit(1)            : EntityQuery<ArmorStand, AtMostOne>
```

Spatial filters and ordering methods such as `.within(...)` and `.nearest()` are
future query variants, not implied by this first plan. Under the selected target
contract, resolving the current `@e[type,tag,limit]` form conservatively consumes the
current position and dimension; `.limit` refines multiplicity but does not erase
those context reads.

The plan may lower directly to a selector when the selected Java target supports
every operation. Arbitrary `.where(|entity| user_code)` predicates are deferred:
they would imply general runtime filtering and could conceal command sequences or
forks that a selector cannot represent. Compiler explanations and dumps must show
the exact selector or fallback recipe chosen for a query.

Nominal kinds compose semantic capabilities rather than forming a deep inheritance
tree. The intended long-term shape is:

```text
Player       = Entity + Living + Positioned + HasInventory<PlayerInventory>
Zombie       = Entity + Living + Positioned + HasEquipment<MobEquipment>
Chest        = BlockEntity + Positioned + HasInventory<Chest27>
Furnace      = BlockEntity + Positioned + HasInventory<FurnaceSlots>
```

Capabilities are inferred by default and become source-visible when required by a
generic constraint or diagnostic. Struct-like projections such as
`player.inventory`, `chest.inventory`, and `zombie.equipment` are typed semantic
views; they do not assert that all owners share a physical representation. A common
internal `HasSlots<Layout>` concept may support both inventory and equipment while
preserving useful domain names and layout-specific members such as `hotbar[0]`,
`head`, or `fuel`.

Stage 7 records this composition model but implements only capabilities consumed by
its first command recipe. It does not add inventories, equipment, generic capability
syntax, or a general object layout. Their source spelling remains deliberately
provisional.

### Minecraft execution context

The semantic context tracks only facts commands can require:

```text
ExecutionContext {
  executor
  position
  rotation
  dimension
  anchor
}
```

Each component distinguishes known, inherited/ambient, and unavailable state where
that difference matters. Source checking rejects an operation whose required
component is unavailable. It does not invent an executor or silently select `@s`.
Modifier checking is one pure, source-ordered fold from the incoming context to the
final context. This is a transfer function, not a dataflow join: later modifiers
consume the exact frame produced by earlier ones. The verifier replays the same
closed transfer rules and rejects cached final context, provenance, or bounds that do
not match the ordered modifier inventory. The current Core verifier may begin this
replay at ordinary function entry because `.as` does not consume a previously
established executor. Before `.at_executor()` or another modifier can depend on an
enclosing scope, Core verification must instead carry symbolic incoming context
requirements or occurrence-aware context; replaying every nested scope from absolute
function entry would reject valid composition.

There are two context forms:

1. A receiver-scoped operation such as `player.say(text)`. Its semantic operation
   explicitly owns `player` as an entity receiver, and the target recipe emits a
   scoped `execute as <player> run ...`. Context cannot leak after the operation.
2. A contextual `run` statement whose modifier chain prefixes a structured lexical
   region. The following is forward design showing the intended composition; the
   implemented Stage 7C syntax currently supports the `.as(...)` portion only:

   ```text
   if (enabled) run
       .as(players)
       .at_executor()
       .rotated(rotation)
       .in(overworld) |player| {
           player.say("hello");
           update_state();
       }
   ```

   The HIR owns the ordered modifiers, resulting context, body region, captures, and
   result rule. Core derives and verifies invocation bounds when it outlines the
   region into a call-like operation. That operation usually lowers to
   `execute ... run function ...`. `run` is a language contextual-block introducer,
   not an ordinary value or function call.

Context entry/exit is never represented as two flat mutable Core instructions. Flat
enter/exit operations would become unsound under branches, early returns, DCE,
outlining, and scheduling.

The first structured scope is `Void` and cannot yield values out of a
many-entity fork. Reductions and merge semantics require a later explicit design.

### Run scopes are typed compile-time structure

`run` is a compiler-known contextual statement. Each chained method conceptually
produces an immutable plan with an updated context, multiplicity, effect, and outcome
summary. The final block completes the statement. Modifier order is semantic and is
never canonicalized by name.

The Stage 7 surface keeps plans non-first-class by grammar: `run`, zero or more
modifier calls, and the block form one statement. The plan cannot be assigned,
returned, or passed as an ordinary value. This needs only contextual-statement
parsing and one structured HIR node, not a general compile-time evaluator.

When the resulting context has a known executor, an optional Zig-style capture gives
it a local name:

```text
run.as(players).at_executor() |player| {
    player.say("hello");
}
```

The binding is per resulting context and has type `Executor<Player>` in this example.
It is a non-escaping lexical capability proving what Minecraft's current `@s` is,
not an `EntityRef`, selector, UUID, or physical value. Initially it may be used as a
method receiver but not returned, stored, or passed to an ordinary value parameter.
An executor-relative method can therefore emit a bare command inside the outlined
body; the surrounding run scope establishes the executor once.

The capture may be omitted when the body does not need to name the executor. Omitting
it does not remove the executor from Minecraft's execution context or change fork
behavior; it only avoids introducing a source binding.

Context-only transforms do not invent captured values. `at` changes
position/rotation/dimension but does not bind its selected entity as `@s`. The
explicit `.at_executor()` spelling selects the already-current executor for that
transform; it does not introduce a source `self`. A capture is allowed only when the
final context proves an executor. Full-context values or captures remain deferred
until a concrete operation needs one.

Context components carry provenance as well as availability. In particular, `as`
does not change the coordinate frame, `at_executor()` records that position,
rotation, and dimension now correspond to the current executor, and
`positioned(spec)` interprets absolute/`~`/`^` components against the frame produced
by every preceding modifier. Later commands use the transformed frame. Structured
coordinate components and the explicit direct entity-relative policy are
detailed in [`../mcfunction/coordinate-frames.md`](../mcfunction/coordinate-frames.md).

The official Java 26.2 server report exposes these root branches:

| Family | Branches | Semantic role |
| --- | --- | --- |
| context | `align`, `anchored`, `at`, `facing`, `in`, `positioned`, `rotated` | transform selected context components; entity forms may fork |
| executor | `as`, `on` | select a new executor; relation/cardinality determines forks |
| stateful executor | `summon` | create an entity and bind it as executor |
| filter | `if`, `unless` | retain or discard contexts using a typed condition |
| outcome sink | `store` | store the nested command's success or result |
| terminal | `run` | execute the nested command; represented by MDL's leading `run` block statement |

The method surface should cover the server branches without reproducing the raw
Brigadier grammar. For example, typed overloads distinguish `positioned(location)`,
`positioned_as(entity_query)`, and `positioned_over(heightmap)` while preserving the
fact that each maps to the `positioned` family. Conditions cover biome, block/region,
data, dimension, entity existence, function outcome, items, loaded position,
predicate, score comparison/range, and stopwatch range as their semantic types
arrive. Stores distinguish success from result and typed score, storage, entity,
block, and bossbar destinations.

Not every chain method is a harmless setter. `as`, `at`, `positioned_as`,
`rotated_as`, entity-facing, and `on passengers` can multiply contexts. `summon`
mutates the world. `if function` executes nested work. `store` observes the terminal
command outcome. Descriptors and function summaries retain these differences even
though the syntax is uniformly fluent.

Ordinary language `if` and execute filtering remain distinct. `if (condition)` is
normal language control flow and may later be lowered to an efficient target recipe.
`run.if(mc_condition)` and `.unless(mc_condition)` explicitly request Minecraft's
ordered context filter, including its native outcome, work, and cost behavior.

`store` before an arbitrary `Void` block is not automatically meaningful. It is
accepted only when the terminal command/block defines the Minecraft success/result
being stored; otherwise the checker requires an explicit outcome-producing form.
This prevents the result of an outlined `function` call from accidentally becoming
the language meaning of a block.

The current fixed-slot backend remains single-context and non-reentrant. Stage 7 can
represent and diagnose a many-context execute plan, but its first executable slice
supports only plans proven to invoke the outlined body at most once. General forked
bodies and escaping values require the Stage 8 calling-convention work; bounded
scheduling remains Stage 9.

An `AtMostOne` query that matches nothing follows Minecraft redirect semantics and
skips the run body. Absence is not an implicit error or default entity. A separate
query/optional API will expose absence when source code needs to branch on it.

### Function behavior summaries

Every function has an inferred closed behavior summary alongside its ordinary value
signature:

```text
FunctionBehavior {
  required_ambient_context
  world_effect
  observable_effect
  fork_bound
  transitive_work
  contains_unsafe_unknown
}
```

The summary is computed monotonically over the call graph to a fixed point. Users do
not write effect claims in Stage 7, and the summary is not yet a first-class function
type because Stage 7 has no higher-order functions. Calls compose the inferred facts,
and datapack exports retain them. The Stage 7 source checker validates the exact
lexical executor capture used by `Say`; general caller-supplied executor-capability
syntax and its source call-site rules remain deferred.

The initial domains are finite and deliberately qualitative:

- each context component is `None`, one precise `Required` value, or `Unknown`;
- world effects form the closed read/write diamond with a separate `Unknown` top;
- observable effects are `None`, `Observable`, or `Unknown`, independently of world
  reads and writes;
- forks are `None`, `Finite(n)`, `NoFiniteUpperBound`, or `Unknown`, preserving the
  distinction between no redirect and a one-context redirect; and
- work is `Zero`, `Finite`, `NoFiniteUpperBound`, or `Unknown`. Exact physical command
  cost remains target analysis, not source behavior inference.

Checked HIR owns one dense summary table indexed by source function. Inference uses
an iterative SCC decomposition, evaluates the condensation graph callee-first, and
confines deterministic sparse worklists to recursive components; HIR verification
recomputes the table. Outlined Core helpers do not receive copied source summaries.

Core independently recomputes ambient-context requirements after outlining and after
optimization. Direct semantic operations contribute requirements, ordinary calls
propagate them, and run scopes reverse-transfer the outlined body's requirements
through ordered modifiers. A zero-modifier nested scope preserves an inherited
requirement; `.as(kind)` discharges a matching executor requirement. The unoptimized
source-to-Core boundary differentially compares mapped HIR and Core requirements,
while the optimized Core result is published per generated `LoweredFunction`. The
pack-wide `ExecutionContract` remains limited to activation and command-limit facts.

Run scopes transfer a callee summary through their ordered context transform. A
component established by `.as`, `.at_executor()`, `.in`, or another modifier can
discharge the corresponding ambient requirement of the outlined body; requirements
that remain unavailable or merely inherited continue outward. Naively unioning the
callee's ambient requirements into the caller would reject valid scopes and erase the
reason the scope exists.

For the current `.as(query)` modifier, reverse transfer discharges the body's
executor requirement, preserves its other frame requirements, and adds the query's
position and dimension requirements. The query is a world read. Repeated `.as`
modifiers multiply each ordered prefix's cardinality, while the function fork summary
records the maximum individual reachable redirect expansion rather than pretending
it is an exact invocation count. Redirects in separate nested commands join by that
maximum; they do not multiply as though they were one execute chain.

Native bulk operations and context forks remain different. A single command that
affects many entities may have one invocation with bulk semantics; `execute as @e`
creates one execution branch per selected entity. Cost and scheduling analysis must
not collapse those cases.

Likewise, scheduling is never a transparent escape from either hard limit. A
scheduled continuation starts a new root but loses executor and frame context and
exposes intermediate world state across ticks. Only an explicitly yielding/task
region may later choose that lowering; Stage 7 synchronous run scopes remain one
atomic source action for scheduling purposes.

### Values intentionally deferred

Stage 7C may name semantic types such as `MessageLiteral`, `Text`, `Location`,
`CommandSuccess`, and `CommandResult`, but implements only the representations
required by 7D's first vertical slice. `say` consumes Minecraft's `message` argument,
so its compile-time `MessageLiteral` is not misrepresented as structured JSON
`Text`. General storage/NBT values, dynamic entity handles, lists, structured text
composition, and representation alternatives remain Stage 8.

In particular, Stage 7 defines the distinction between `EntityRef`, `EntityQuery`,
and `Executor` but does not invent a general physical entity-reference handle. The
first runnable slice needs only a compiler-known static query and a scoped executor
capture. Arbitrary runtime `EntityRef` producers and their representation bridges
remain Stage 8.

## Stage 7D: method-first commands and target recipes

### Source API rule

Operations whose primary subject is an entity/value are methods:

```text
player.say("hello")
player.teleport(destination)
```

`player.say` means the player is the executor of Minecraft's `say` command. It is not
an alias for `tellraw player ...`. The semantic receiver is explicit even when the
target spelling needs an `execute as` wrapper.

Dotted syntax alone does not determine how the receiver appears in the target
command. A source-method rule classifies and normalizes the receiver before the
target-independent semantic descriptor is consulted:

```text
EntityRef + say       -> EstablishExecutorThen(Say) // future sugar
Executor + say        -> CurrentExecutor(Say)       // Stage 7 slice
EntityRef + teleport  -> CommandTarget(Teleport)
ordinary value method -> ValueOperand(operation)
```

`EntityRef.say` normalizes to a one-statement run scope that establishes the receiver
as executor, then performs the ambient `Say` semantic operation. `Executor.say`
performs that same ambient operation directly. Both retain one compiler-owned `Say`
meaning; the former merely contributes a scope. In contrast,
`EntityRef.teleport(location)` uses the entity as a command target and must not gain
an unnecessary `execute as` solely because it is method-shaped.

World-level constructors and queries live under a reserved compiler namespace:

```text
mc.players(...)
mc.location(...)
mc.try_set_block(location, block)
```

Method and namespace convenience spellings, if both exist, resolve to one
`MinecraftSemanticKey`. They never duplicate descriptors or lowerings. The initial
surface can remain deliberately small while literal unsafe commands provide an
escape hatch for missing coverage.

### One semantic registry

The compiler owns one declarative registry of normalized typed Minecraft meaning. A
descriptor contains independent fields:

```text
MinecraftSemanticDescriptor {
  key
  semantic_operands
  semantic_results
  static_attributes
  ambient_requirements
  world_effect
  observable_effect
  fork_behavior
  work_behavior
  outcome_behavior
  validator_kind
  documentation
}
```

Source-method lookup is a separate small closed table containing spelling, receiver
constraints, and normalization. This keeps one normalized `Say` meaning even when a
future `EntityRef.say` spelling contributes an implicit exact-one scope.

Start with a normal Rust declarative table/macro and closed enums. Do not add an
external schema language or code generator until duplication demonstrates a need.
The registry generates or drives:

- method/namespace lookup;
- source signature verification;
- HIR/Core descriptor verification;
- function behavior summarization;
- exhaustive descriptor tests; and
- user-facing API/support documentation.

Irregular command families may use handwritten checking and lowering behind the
same semantic key. The goal is a single source of semantic truth, not forcing every
Minecraft command into an unnaturally uniform record.

Brigadier/server command reports are compatibility-audit inputs, not the language
schema. Their syntax tree cannot express MDL's types, cardinality, effects, context,
or cross-version semantic equivalence.

### Target recipe selection

Target lowering is a separate closed, instance-aware lookup:

```text
select_recipe(JavaEditionTarget, VerifiedMinecraftOperation) -> MinecraftRecipeId
```

A recipe consumes represented operands and emits only verified structured Minecraft
IR. It records exact context/fork/outcome behavior and predicted physical cost, which
is reconciled with the constructed target program. Unsafe raw fragments bypass this
matrix and carry no portability promise.

Selection examines the verified operation instance so later legality may depend on
types or attributes. Stage 7 has one semantic key, one target, and one supported
pair, so the match is total. Do not invent a fake target or fake operation just to
exercise an unreachable unsupported-pair diagnostic. Native versus emulated recipes
and their policy are deferred until a second real target makes the product partial.
The compiler still never weakens source semantics merely to claim support.

### Multiple Minecraft and datapack versions

Multiple targets are a long-term feature, not a Stage 7 implementation item. Stage 7
preserves only the boundary needed to avoid blocking them later:

- one compilation selects one exact `JavaEditionTarget`;
- the semantic HIR/Core key is target-independent;
- `TargetSpec` stores compile-relevant facts: exact game version, datapack format,
  pack layout/serialization rules, hard-limit defaults, the required conformance Java
  runtime version, and closed capabilities;
- the conformance harness resolves the actual Java executable and owns the pinned
  server JAR/hash and measured behavior evidence rather than making test-artifact
  identity part of `TargetSpec`;
- datapack format compatibility does not imply command-semantic compatibility;
- command recipes are selected through the exact target rather than embedded in HIR;
  and
- there is no vague `latest` target.

Mojang's major/minor pack-format compatibility is useful artifact metadata, but it
cannot replace an exact game target. Command syntax and behavior may change while a
pack-format range remains compatible.

The existing `JavaEditionTarget::V26_2` and `TargetSpec` are the complete Stage 7
target set. Add capability/recipe queries only when the first typed operation
consumes them. Do not implement emulation policy, compatibility ranges, or a second
target merely to exercise an abstraction.

### First vertical command slice

The first runnable typed scope is small but semantically revealing:

```text
run.as(mc.entities(ArmorStand).with_tag("stage7").limit(1)) |speaker| {
    speaker.say(MessageLiteral)
}
```

The static query is `EntityQuery<ArmorStand, AtMostOne>`. If it matches, the capture
is one exact `Executor<ArmorStand>` for that invocation; if it does not, the block is
skipped. This slice proves method resolution, query cardinality, scoped executor context,
external Core linkage, target recipe selection, structured `execute as`,
effects/cost reporting, provenance, and official-server observation without needing
a connected client or pretending a runtime entity handle already exists.

The Stage 7D server fixture summons one tagged armor stand, invokes the exported
source function, and asserts the executor-attributed server log. Empty-match and
unbounded-query fixtures prove skip and explicit-fork behavior. The semantic
`EntityRef.say` shorthand is documented now but need not be executable until Stage 8
provides an actual entity-reference producer. Teleport is a reasonable next family
only after location semantics and entity target representation are defined.

## Cross-stage ownership

Stage 7 deliberately leaves these responsibilities elsewhere:

- **Stage 8:** scalar physical realizations and calling-convention/reentrancy changes;
  the broader entity/dynamic/list inventory is now follow-on client work described by
  [`stage-8-plan.md`](stage-8-plan.md);
- **Stage 9:** bounded loops, explicit iteration lowering, static continuations, and
  multi-tick scheduling using Stage 7 behavior summaries;
- **Stage 10:** language macros, typed command templates, and Minecraft function
  macros;
- **Stage 11:** precise Core effect exploitation, global recipe selection,
  specialization, and optional equality saturation; and
- **Stage 12:** manifests, dependency resolution, stable package/resource ABI,
  version distribution policy, and broad ecosystem compatibility.

## Stage exit criteria

Stage 7 is complete only when:

1. the same single-source program still compiles through the package adapter;
2. the rooted module graph, local dependency names, visibility, cycles, and
   deterministic normalization pass package tests;
3. external Core operations survive every verifier, optimizer, printer, editor,
   lowering, and differential audit conservatively;
4. literal unsafe commands travel end to end and remain unknown barriers;
5. the entity-reference/query/executor semantic distinction exists, while query and
   captured-executor behavior plus cardinality errors are source-visible without
   claiming a runtime `EntityRef` producer;
6. receiver-scoped context does not leak and a many-entity fork cannot masquerade as
   a scalar entity call;
7. HIR behavior and Core ambient-requirement summaries converge deterministically
   through call cycles and agree at the unoptimized source/Core boundary;
8. the first typed method lowers only through a selected structured target recipe;
9. target-dependent invalid operation instances fail during preflight with source
   provenance and no partial target output;
10. reports expose semantic key, recipe, fork/context behavior, and physical cost;
11. deterministic source/HIR/Core/target/trace/pack differentials pass; and
12. the pinned Java 26.2 server proves both the typed command and unsafe boundary.

## Frozen for this slice; extensible later

The first slice keeps the current Zig-like dotted spelling, requires an explicit
`run.as(query)` fork for possibly-many execution, and keeps generated export names
build-local. Refining surface spelling, adding explicit iteration syntax, and
stabilizing cross-datapack resource names are later changes, not Stage 7 blockers.

## Primary references

- [Zig language reference: compilation model, root module, and `@import`](https://ziglang.org/documentation/master/)
- [Zig build system: root modules and dependency names](https://ziglang.org/learn/build-system/)
- [Rust compiler guide: name resolution](https://rustc-dev-guide.rust-lang.org/name-resolution.html)
- [Rust Reference: namespaces and staged name resolution](https://doc.rust-lang.org/reference/names/name-resolution.html)
- [Rust Reference: modules, paths, and visibility](https://doc.rust-lang.org/reference/items/modules.html)
- [Rust Reference: visibility and privacy](https://doc.rust-lang.org/reference/visibility-and-privacy.html)
- [OCaml manual: compilation units and modules](https://ocaml.org/manual/5.5/compunit.html)
- [GHC User's Guide: separate compilation and source cycles](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/separate_compilation.html)
- [MLIR: symbols and symbol tables](https://mlir.llvm.org/docs/SymbolsAndSymbolTables/)
- [MLIR `func` dialect: symbolic direct calls and function visibility](https://mlir.llvm.org/docs/Dialects/Func/)
- [MLIR LLVM dialect: linkage is distinct from symbol visibility](https://mlir.llvm.org/docs/Dialects/LLVM/#linkage)
- [MLIR: operation definition specification](https://mlir.llvm.org/docs/DefiningDialects/Operations/)
- [MLIR: call/callable operation interfaces](https://mlir.llvm.org/docs/Interfaces/#callinterfaces)
- [MLIR: non-SSA symbol uses and `SymbolUserOpInterface`](https://mlir.llvm.org/docs/SymbolsAndSymbolTables/#referencing-a-symbol)
- [MLIR: side effects and speculation](https://mlir.llvm.org/docs/Rationale/SideEffectsAndSpeculation/)
- [MLIR: structured control-flow regions](https://mlir.llvm.org/docs/Dialects/SCFDialect/)
- [MLIR SPIR-V: versions, capabilities, target environments, and conversion legality](https://mlir.llvm.org/docs/Dialects/SPIR-V/)
- [LLVM: convergent operations and explicit execution dependence](https://llvm.org/docs/ConvergentOperations.html)
- [LLVM Language Reference: inline assembler expressions](https://llvm.org/docs/LangRef.html#inline-assembler-expressions)
- [LLVM Language Reference: `callbr` makes opaque branch destinations explicit](https://llvm.org/docs/LangRef.html#callbr-instruction)
- [Koka book: effect types and inferred effects](https://koka-lang.github.io/koka/doc/book.html)
- [Cranelift: declared external function references and signatures](https://docs.rs/cranelift-codegen/latest/src/cranelift_codegen/ir/extfunc.rs.html)
- [Cranelift IR: typed direct calls through function-preamble references](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md#function-calls)
- [Cranelift's compact closed IR value-type representation](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/codegen/src/ir/types.rs)
- [Cranelift's typed dense primary entity maps](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/entity/src/primary.rs)
- [Cranelift module `Linkage`: local versus exported declarations](https://docs.rs/cranelift-module/latest/cranelift_module/enum.Linkage.html)
- [rustc guide: centralized MIR visitor and mutable-visitor traversal](https://rustc-dev-guide.rust-lang.org/mir/visitor.html)
- [rustc guide: typed THIR makes types and implicit adjustments explicit before MIR](https://rustc-dev-guide.rust-lang.org/thir.html)
- [rustc's generic closed type-kind representation](https://github.com/rust-lang/rust/blob/main/compiler/rustc_type_ir/src/ty_kind.rs)
- [Rust Reference: inline assembly contracts and optimizer permissions](https://doc.rust-lang.org/reference/inline-assembly.html#options)
- [Zig AIR: closed instruction tags for calls and inline assembly](https://github.com/ziglang/zig/blob/master/src/Air.zig)
- [Zig compiler intern pool for its large structural type/value universe](https://github.com/ziglang/zig/blob/master/src/InternPool.zig)
- [Zig language reference: `asm volatile`, inputs, outputs, and clobbers](https://ziglang.org/documentation/master/#Assembly)
- [GHC primops: `can_fail` and `has_side_effects` constrain optimization](https://gitlab.haskell.org/ghc/ghc/-/blob/master/compiler/GHC/Builtin/PrimOps.hs)
- [GHC roles: nominal identity is distinct from representation equality](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/exts/roles.html)
- [GHC linear types: value-use multiplicity is separate from runtime fan-out](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/exts/linear_types.html)
- [Zig language reference: explicit compile-time branch quota](https://ziglang.org/documentation/master/#setEvalBranchQuota)
- [Mojang Brigadier: redirects, modifiers, and forked command nodes](https://github.com/Mojang/brigadier)
- [Mojang Brigadier `StringReader`: reviewed unquoted-word alphabet and quoted-string escaping](https://github.com/Mojang/brigadier/blob/master/src/main/java/com/mojang/brigadier/StringReader.java)
- [Mojang 18w02a: `execute as` and per-entity execution](https://feedback.minecraft.net/hc/en-us/articles/360004167991-Minecraft-Java-Edition-Snapshot-18W02A)
- [Minecraft Java 1.21.9: major/minor datapack versions and compatibility metadata](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-9)
- [Minecraft Java 26.2 pre-release 4: datapack format 107.1](https://feedback.minecraft.net/hc/en-us/articles/46393304366349-Minecraft-Java-Edition-26-2-Pre-release-4)
- [Minecraft Java 26.2 release and official server download](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
- [Minecraft Java 1.20.3: `return run` propagates a command result out of a function](https://feedback.minecraft.net/hc/en-us/articles/21968446892173-Minecraft-Java-Edition-1-20-3)
- [Minecraft Java 1.19.4: `execute on`, `positioned over`, `summon`, dimension, and loaded conditions](https://feedback.minecraft.net/hc/en-us/articles/13987663727757-Minecraft-Java-Edition-1-19-4)
- [Minecraft snapshot 25w41a: stopwatch condition](https://feedback.minecraft.net/hc/en-us/articles/40290141596301-Minecraft-Java-Edition-Snapshot-25w41a)
- [Sandstone documentation: typed commands and scoped execute builders](https://sandstone.dev/)
- [Trident documentation: command-oriented language and verbatim escape](https://energyxxer.com/trident/)
