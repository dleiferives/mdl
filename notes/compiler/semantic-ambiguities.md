# Compiler Semantic Ambiguities and Unknowns

This is the durable ledger for target behavior or program facts that the compiler
cannot determine from the verified input alone. Add an entry when implementation
work encounters an ambiguity, runtime-dependent fact, target-version uncertainty,
or intentionally unsupported semantic surface.

These categories must not be conflated:

- **undefined behavior** is behavior the MDL language contract deliberately leaves
  without requirements;
- **implementation-defined behavior** has one compiler-selected behavior that must
  be documented;
- **unspecified behavior** permits a documented set of behaviors without promising
  which member occurs;
- **compile-time unknown** has defined runtime semantics, but the compiler lacks the
  runtime state or assumptions needed to select an exact result;
- **unsupported semantics** means the compiler intentionally returns a typed
  `Unknown` fact instead of inventing a model.

For every entry, record the conservative rule used today and the evidence or new
contract required to make it more precise. A compiler result is sound when every
conforming runtime behavior remains inside the reported abstract result; sound does
not mean that every bound is exact.

## A-001 — Runtime multiplicity of a Minecraft call site

**Status:** compile-time unknown; Phase-1 policy decided.

One structured call site is not one runtime invocation. For example:

```mcfunction
execute as @e run function mdl:worker
```

has one syntactic `function` call site. When the ordinary redirect's runtime fork
check admits the expansion, its body is invoked once for each selected entity; at or
above the configured fork limit that redirect is rejected instead. The compiler does
not generally know the gamerule value, selected population, score/NBT guards, prior
world mutation, executor presence, external datapack behavior, or a recursive trip
count. Exact runtime multiplicity is therefore not a property of the call graph.

Phase 1 keeps the following facts separate:

1. Each occurrence of `InternalCallSite` is exact typed syntax/control structure:
   target plus ordinary, `return run`, or function-condition role. Equal descriptors
   remain distinct sites by their ordered occurrence; raw and external calls do not
   fabricate owned sites.
2. The SCC graph projects reachable site occurrences to unique caller-to-owned-function
   connectivity. A tag remains a tag in the local site inventory but expands to its
   owned function members for this projection; roles and duplicate sites are not
   retained because reachability/SCC solving does not consume them. `graph_edges`
   counts this projected relation, not sites or invocations.
3. `TargetExecutionCostReport` retains aggregate invocation `CountBound`s. These may
   be exact, finite ranges, above the analysis cap, `NoFiniteBoundProven`, or
   `Unknown`.
4. Per-edge dynamic counts are deferred until a concrete optimization requires them.
   Such an analysis must declare its environment assumptions or consume empirical
   profile data; it must not reinterpret syntactic occurrence count as frequency.

Likewise, a retained structural SCC is a conservative syntactic cycle. Outcome
feasibility may prove that its recursive edges cannot execute for a particular root,
so `is_cyclic()` does not itself imply positive or unbounded runtime cost.

Examples of sound aggregate facts include an unconditional reached call as exactly
one local invocation, a guarded or `@s`-forked call as zero-or-one, an unbounded
selector as having no proven finite cardinality, and raw/external behavior as
`Unknown`. These are abstract bounds, not predictions of one server state.

The regression
`unbounded_execute_call_keeps_structure_separate_from_runtime_multiplicity` in
`crates/mdl-compiler/src/analysis/minecraft/analyze.rs` pins the distinction: the
fixture has one retained call site and one projected graph edge, while its root
function-invocation bound has lower bound one (the root itself) and no proven finite
upper bound because the nested call is repeated for the runtime `@e` cardinality.
The same test separately pins a direct single-context call weight of one and a
selector-aware command-outcome bound with no proven finite upper value; neither local
representation is allowed to impersonate the other.

The ignored official-server regression `command_limit_boundaries_match_vanilla_26_2`
in `crates/mdl-test/tests/command_limits.rs` pins the literal nested-call shape as
well. With five tagged, command-visible armor stands,
`execute as @e[...] run function mdl_limit:multiplicity_worker` increments the worker
counter exactly five times. This prevents a future structural call-graph cleanup from
silently reintroducing “one call site means one invocation.”

This separation follows established compiler structure:

- MLIR call-graph nodes retain deduplicated target/kind edges, while its dataflow
  framework propagates separate lattices through call control flow:
  <https://mlir.llvm.org/doxygen/CallGraph_8h_source.html> and
  <https://mlir.llvm.org/docs/Tutorials/DataFlowAnalysis/>.
- LLVM call-graph records associate an edge with a call instruction and callee; profile
  counts are optional results of separate frequency/profile analysis:
  <https://llvm.org/doxygen/classllvm_1_1CallGraphNode.html> and
  <https://llvm.org/docs/doxygen/classllvm_1_1BlockFrequencyInfo.html>.
- Cranelift represents direct calls as typed instructions/function references rather
  than claiming a runtime count:
  <https://docs.rs/cranelift-codegen/latest/cranelift_codegen/ir/instructions/index.html>.
- GHC demand analysis is the relevant static-count analogue: it deliberately uses
  conservative evaluation-cardinality intervals such as zero, one, or no useful
  upper information, relative to an enclosing call:
  <https://ghc.gitlab.haskell.org/ghc/doc/users_guide/using-optimisation.html> and
  <https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/src/GHC.Types.Demand.html>.
- OCaml Flambda costs observed removable operations at a call site and explicitly
  uses heuristics when hot/cold information is unavailable; it does not turn a static
  call occurrence into an exact execution frequency:
  <https://ocaml.org/manual/5.1/flambda.html>.

**Future precision trigger:** add a separate analysis only when a pass needs a
per-edge fact that aggregate target cost cannot answer. Its cache key must include
all declared world/cardinality assumptions, or its result must be explicitly
profile-derived.

## A-002 — Which entities contribute to selector cardinality

**Status:** runtime-dependent visibility; empirically confirmed on Java 26.2.

The abstract cardinality of `@e` is not the number of matching entities serialized
anywhere in the save. It is the number of entities visible to command evaluation in
the current runtime state, including chunk loading/activation and selector filters.
Consequently, the compiler models an unbounded selector as a fresh runtime-dependent
cardinality at each command. It must not reuse one symbolic `|@e|` value across world
mutations or assume entities in unloaded chunks participate.

A no-client black-box test used the official Java dedicated server 26.2, OpenJDK
25.0.3, and extracted server JAR SHA-256
`183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8`. In a fresh
flat world, a controlled chunk was force-loaded before summoning tagged armor stands.
The fixture independently measured the selected population, reset a scoreboard, and
incremented it under the selector:

```mcfunction
execute store result score #selected mdl_exec run execute if entity @e[...]
scoreboard players set #body mdl_exec 0
execute as @e[...] run scoreboard players add #body mdl_exec 1
```

Populations of zero, three, and seven produced exactly zero, three, and seven body
increments. Bare `@e` independently selected ten visible entities and produced ten
increments. An earlier summon into a non-active chunk produced zero immediate matches,
confirming the visibility caveat. Therefore one incoming context obeys:

```text
body executions = runtime selector cardinality among command-visible entities
```

Nested input contexts multiply this transfer per surviving input context; they do not
turn the runtime population into a compile-time constant.

## A-003 — Local command completion versus solved callee behavior

**Status:** representation ambiguity; Phase-1 policy decided.

`CommandStepCost` is deliberately local: its syntax weights and outcome transfer do
not recursively expand or depend on a callee summary. This means a structured
ordinary function or function-tag call must retain both result-bearing continuation
(`Continue`) and resultless continuation (`NoResult`) until interprocedural solving;
one syntactic call cannot borrow the current body of its target and pretend that is a
local fact. Raw commands similarly retain `Continue`, `NoResult`, coarse
`Return(UnknownInteger)`, and `Fail` under their stable unknown barrier.

The closed outcome order is deterministic. Local `outcome_costs` may split a coarse
`Return(UnknownInteger)` into zero/nonzero alternatives, but it must not silently omit
a syntax-local possibility. Whole-root summaries may later narrow behavior using the
solved graph. Analysis-limit fallback preserves the syntax-local classes and marks
their dynamic metrics `Unknown(AnalysisLimit)` rather than publishing partially
solved callee facts.

This separation prevents a change to a callee body, a recursive SCC, or an analysis
budget from changing what the compiler calls an exact local syntactic fact.

## A-004 — Equality at the two command-limit gamerules

**Status:** target-specific boundary semantics; Java 26.2 policy decided.

The two integer gamerules do not use the same safety comparison, and the configured
sequence value is not always the effective quota:

```text
effective_sequence_quota = max(1, max_command_sequence_length)
sequence is safe when total_sequence_operations <= effective_sequence_quota
positive ordinary checked-redirect expansion is safe when
    maximum_individual_checked_redirect_expansion
    < max_command_forks
```

Java 26.2 allows the operation at the effective sequence boundary, so a root that
naturally finishes at equality is compatible. With configured values zero and one,
one operation ran and a second did not. Fork accumulation instead rejects the redirect
when `accumulated_output_size + new_size >= max_command_forks`; a value of four
therefore permits at most three contexts at the checked stage. Treating both as
ordinary inclusive maxima would miscompile the fork equality case.

Fork zero is a special abstract boundary. Direct commands do not encounter a redirect
check and still run. An ordinary checked redirect can fail even when it produces zero
outputs, so the numeric expansion alone cannot make an arbitrary zero-output redirect
safe. The current comparison avoids that false proof by consulting both retained
facts. Exact zero checked expansion is within at configured fork zero only when the
execute-stage bound is also exact zero. A solved zero-output execute path remains
`MayExceed`, as does expansion `[0, 1]`; exact positive expansion is
`ProvenExceeds`. A-007 records why custom function conditions do not themselves add
checked expansion and why the execute-stage fallback is intentionally imprecise for
them at fork zero.

The exact runtime boundary evidence and root/fork distinction are retained in
[`../mcfunction/command-limits-and-multi-tick.md`](../mcfunction/command-limits-and-multi-tick.md).

`CommandLimitStatus` therefore compares the two metrics with separate functions and
keeps `ProvenWithin`, `MayExceed`, `ProvenExceeds`, no-finite-bound, and unknown states
distinct. The arithmetic cap remains above each assumption so capped bounds cannot
hide either equality boundary.

## A-005 — Documented gamerule domain versus the pinned 26.2 server

**Status:** target-version discrepancy; Java 26.2 executable behavior pinned.

Mojang's 1.21.11 registry notes document `minecraft:max_command_forks` with minimum
one and `minecraft:max_command_sequence_length` with minimum zero. The final official
Java 26.2 server instead registers **both** with minimum zero. Its shared integer-rule
constructor supplies `Integer.MAX_VALUE` to both Brigadier parsing and persisted-value
decoding, giving the exact configured domain `0..=2_147_483_647`.

This is neither MDL undefined behavior nor permission to choose whichever source is
convenient. `JavaEditionTarget::V26_2` follows the pinned executable target:

- `CommandLimitAssumptions::new` accepts zero for both values and rejects either value
  above `2_147_483_647`;
- target defaults pass through that same smart-construction invariant;
- the configured values remain visible in contracts and reports, while effective
  sequence-zero behavior is derived separately;
- future targets must state their own registered domain and execution semantics
  instead of inheriting this discrepancy silently.

Evidence came from the official 26.2 dedicated-server distribution. The extracted
server JAR used for bytecode inspection and black-box tests has SHA-256
`183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8`.
The inspected methods were `GameRules.registerInteger`, `GameRules.<clinit>`,
`Commands.executeCommandInContext`, and `BuildContexts.execute`.

References:

- Mojang's documented 1.21.11 registry limits:
  <https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-11>
- Mojang's Java 26.2 release and official server distribution:
  <https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2>

## A-006 — Observable meaning of a zero-output redirect at fork limit zero

**Status:** target behavior partly observed; compiler remains conservative.

Java 26.2's `BuildContexts` guard compares the accumulated and newly produced redirect
contexts with `>= fork_limit`. At configured fork zero, this guard can therefore fire
even when the new redirect result is empty. A black-box `execute as` over zero matching
entities ran no body, but that final state is also the natural result with a permissive
fork limit. The fixture did not yet distinguish command feedback, stored success/result,
or enclosing `return run` behavior between those two causes.

This is defined Minecraft runtime behavior that the current evidence has not fully
characterized, not MDL undefined behavior. A maximum-expansion bound of exact zero can
arise after interprocedural feasibility proves a reached execute path produces no
contexts. Consequently, at configured fork zero the compiler reports:

- `ProvenWithin` only when both expansion and reached execute-stage counts are exact
  zero;
- `MayExceed` when expansion is exact zero but an execute stage may be reached;
- the ordinary positive-bound classifications for possible or proven expansion.

This deliberately gives up a little precision instead of treating “zero contexts” as
proof that no server guard ran. To refine it, add a separate checked-redirect-presence
effect or prove with official-server fixtures that all observable structured outcomes
are equivalent. The required fixture should compare fork zero and one under direct
execution, `execute store success`, `execute store result`, and `return run`, including
server diagnostics.

## A-007 — Function conditions bypass the ordinary execute fork guard

**Status:** target-specific exception confirmed; remaining fork-zero precision is a
compile-time analysis limitation.

Java 26.2 does not send every `execute` modifier through the same fork-limit check.
`execute if function` and `execute unless function` are custom modifier executors.
`BuildContexts.execute` dispatches that custom path and returns before its generic
`accumulated_output_size + new_size >= max_command_forks` guard. The function-condition
continuation is queued without an equivalent guard. A true condition can therefore
preserve one context and run its body even when `max_command_forks` is zero.

This was confirmed on the official 26.2 server at configured fork values zero and
one. A direct body and a true `if function` body each ran once. In the same fixture,
one-context `as`, `in`, `store result`, and a passing ordinary `if score` modifier all
ran zero bodies; a zero-match `as` also ran no body. Thus “every execute stage is a
fork-checked expansion” is false even though all of these forms remain structured
execute stages for census and control-flow purposes.

The Phase-1 compiler rule is:

- `maximum_chain_expansion` means the maximum expansion at an **ordinary
  fork-checked redirect**, not every context transfer;
- a custom function condition still contributes its function invocation, outcome
  flow, and execute-stage census, but its surviving context does not itself add a
  checked expansion;
- any ordinary modifier in the same or nested chain is still modeled as checked.

The root report currently retains checked expansion and total execute-stage bounds,
but not a distinct “ordinary checked redirect reached” effect. Consequently, at fork
zero a custom-only condition conservatively receives `MayExceed`: exact checked
expansion is zero, but the generic execute-stage fallback cannot prove that every
possible reached stage is custom. This is a loss of static precision, not Minecraft or
MDL undefined behavior and not an unsafe `ProvenWithin` claim. Add a separate checked-
redirect-presence bound only when a real consumer needs the extra fork-zero precision.

Evidence used the official extracted server JAR with SHA-256
`183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8`, inspecting
`ExecuteCommand.register`, `BuildContexts.execute`, and
`scheduleFunctionConditionsAndTest`, plus the ignored automated vanilla fixture in
`crates/mdl-test/tests/command_limits.rs`.

Reference: Mojang's Java 26.2 release and official server distribution:
<https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2>.

## A-008 — Definition site and identity of optimizer-materialized constants

**Status:** implementation-defined compiler behavior; Phase-1 policy decided.

An SSA rewrite such as `old -> Constant(7)` preserves the program's value but does not
uniquely determine where the new constant instruction lives, which origin it carries,
or whether equal literals share an identity. These choices affect dominance,
diagnostics, stable IDs, deterministic dumps, and later local optimizations even though
they do not change the source-level result.

Stage 5 uses this fixed policy:

- every nontrivial mapped source and every final `Existing` endpoint must have an
  attached definition; there is no executable definition site for detached history;
- an instruction-result constant is inserted immediately before its defining
  instruction and inherits that instruction's origin;
- a block-parameter constant is inserted after conceptual parameters and before the
  first instruction, inheriting the parameter origin;
- sites are planned in block layout and definition order with `ValueId` as the stable
  tie-breaker, independently of input map iteration;
- a mixed replacement chain ending in a constant creates a distinct constant
  instruction/value for every source actually submitted to the editor; equal literals
  are not implicitly shared. Pass-private virtual mappings whose only post-projection
  uses are inside the planned erase set are filtered first and allocate no temporary
  constant/history;
- duplicate source keys, self mappings, and longer cycles are rejected before mutation;
  mappings are simultaneous and path-compressed rather than interpreted sequentially;
  and
- detached historical uses are not rewritten.

This is an internal compiler contract, not permission for nondeterministic output. A
future IR that gives constants a canonical entry-block home may deliberately revise the
policy, but must re-prove dominance/provenance behavior and update deterministic tests.
Cranelift's split between dense data-flow identities and executable layout is useful
precedent for keeping identity and placement separate:
<https://raw.githubusercontent.com/bytecodealliance/wasmtime/main/cranelift/codegen/src/ir/dfg.rs>
and
<https://raw.githubusercontent.com/bytecodealliance/wasmtime/main/cranelift/codegen/src/ir/layout.rs>.

## A-009 — Meaning of detached Core containers

**Status:** implementation-defined diagnostic-history contract; Phase-1 policy
decided.

Core retains allocated block, instruction, and value records after executable layout
detaches them. It was previously quiet whether those records formed reusable IR, a
replayable pre-optimization snapshot, or merely history. Stage 5 defines them as
**non-executable diagnostic history**:

- attached semantics, use indices, CFGs, and optimization rewrites ignore detached
  operands, terminators, and uses;
- stable IDs, origins, bidirectional value definitions, and raw instruction-container
  ownership remain inspectable;
- every listed `InstId`, including one in a detached block, must be allocated and may
  occur at most once across all block vectors, including at most once within one
  vector;
- an erased instruction may have zero raw container owners; and
- reattachment is not an editor operation. It would require consumer-specific
  reconstruction followed by full verification.

The debug dump is therefore diagnostic-only and is not a serialized Core program.
This mirrors the useful identity/layout distinction in Cranelift without inheriting an
unstated reattachment API. Its verifier likewise treats layout ownership as an
explicit invariant:
<https://docs.rs/cranelift-codegen/latest/cranelift_codegen/verifier/>.

## A-010 — Attribution of corruption under boundary-only verification

**Status:** diagnostic ambiguity; conservative attribution policy decided.

In an optimized compiler build, several function passes may run between verifier
boundaries. If final whole-program verification then finds malformed Core, the last
executed pass is not necessarily the pass that introduced the corruption. Naming it as
the culprit would turn temporal proximity into false evidence.

Optimizer failures keep distinct typed phases: input verification, pass application,
immediate after-step verification, and output verification. A pass/step is attached as
the direct failure context only when that pass returned an error or the configured
after-step verifier rejected its immediate result. Whole-program output-boundary
verification deliberately attaches no function or last-pass culprit: structured
diagnostics locate the findings, while the phase and snapshot show that corruption was
observed only at the boundary. The consumed possibly invalid compilation unit is
dropped in every case; its single byte-capped snapshot is diagnostic-only.

Tests/debug builds normally verify after every executed pass to improve attribution.
Release builds may use boundary verification because the extra scans materially affect
the fractional-second goal. This follows the separation between verification policy
and pass semantics in rustc and MLIR:
<https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_mir_transform/pass_manager.rs.html>
and <https://mlir.llvm.org/docs/PassManagement/>.

## A-011 — Dominance obligations in attached but unreachable Core blocks

**Status:** representation ambiguity; Phase-1 policy decided.

An attached block may be unreachable from entry until an optimizer detaches it. Core
still enforces value existence, operation/terminator contracts, and same-block
definition-before-use there. Only **cross-block dominance** is intentionally undefined
for an unreachable use. `Dominance::UnreachableUse` is not a proof that an arbitrary
definition dominates; it tells consumers that the current entry-rooted CFG imposes no
cross-block execution obligation at that use.

Consequently, a terminator batch is checked against its simultaneous projected CFG. If
an edit makes an old block reachable or adds a bypass path, every attached instruction
and projected terminator use is rechecked before mutation. A locally valid terminator
cannot rely on later value replacement to repair temporarily invalid Core. Non-entry
self-loops remain legal; fusion's narrower self/overlap rules are consumer policy, not
generic Core semantics.

## A-012 — Exact fusion scratch requirement below the classification base

**Status:** resource-policy ambiguity; Stage-5 policy decided.

Straight-line fusion's persistent scratch has two parts: dense facts indexed by every
allocated block/instruction ID, and flattened replacement slots only for parameters of
eligible jump destinations. The second cardinality is not immutable entity metadata.
It depends on entry reachability and exact attached incoming-edge counts, which are the
facts stored in the first part.

An explicit fact-table limit below that classification base creates an unavoidable
choice. The compiler could allocate past the user's limit merely to calculate the
eventual exact total, report a conservative all-attached-parameter upper bound, or stop
at the exact minimum required to classify the CFG. This is not MDL undefined behavior
and does not affect generated-program semantics; it is ambiguity in resource-limit
reporting and retry behavior.

Stage 5 chooses two-stage admission:

1. The base is exactly six allocated-block entries per block plus one raw-owner entry
   per allocated instruction. Its lengths and bytes are checked before allocation.
2. A limit below that base returns unchanged and reports the base requirement. It does
   not allocate beyond the configured limit solely to discover a larger number.
3. Once the base is admitted, fusion classifies the CFG, counts the exact
   eligible-destination parameter slots, and checks the complete base-plus-slot total
   before allocating the replacement table. Failure again returns unchanged, now with
   the exact complete requirement.
4. Region discovery and all IR mutation occur only after both admissions succeed.

The fusion limit tests pin boundaries below the base, at the exact complete total, and
inside region discovery. If callers later require a single exact “raise the limit to
this value” answer even when the base is denied, the API must either permit a separate
unlimited sizing query or redefine the limit to include a conservative parameter-slot
upper bound. It must not silently allocate past the active limit.

## A-013 — Semantic demand versus fixed-recipe physical destinations

**Status:** lowering-design ambiguity; Stage-5E policy decided.

A demanded subset of a multi-result Core instruction does not necessarily match the
set of score destinations required by its current Minecraft command recipe. For
example, one result of `I32AddOverflowing` may be live while the other is unused, but
the Stage 4 recipe writes both the wrapping sum and overflow flag while computing the
demanded result. Treating both SSA results as semantically demanded would keep false
uses alive and corrupt later liveness/coalescing decisions. Omitting the second
destination without changing the recipe would instead make lowering incomplete.

Stage 5 keeps three facts separate:

1. `RuntimeDemand` records semantic demand independently for each Core result. A
   `Pure + Always` instruction with no demanded result may be omitted; one demanded
   result keeps the instruction.
2. The immutable instruction plan records a semantic home only for each demanded
   result and separate typed recipe-temporary destinations for any additional writes
   required by the selected fixed recipe. A recipe temporary is not a
   `ValueAssignment`, block argument, ABI slot, or proof that the corresponding Core
   result is live.
3. Plan verification recomputes the minimum semantic demand from Core, permits a
   conservative demanded superset, and independently checks that every emitted recipe
   has all of its physical destinations. Incomplete demand analysis falls back to the
   conservative Stage 4 set; it never publishes or prunes from partial facts.

Stage 5E does not silently specialize `I32AddOverflowing` into different recipes for
different demanded-result masks. Such specialization may be added later as an
explicit closed recipe with its own emitter, cost, access contract, and verifier
case. Minecraft optimization `None` retains the original one-home-per-reachable-value
layout and byte-identical Stage 4 output.

The separation follows the same general analysis-versus-physicalization boundary as
MLIR One-Shot Bufferize, which first records SSA-based in-place decisions and then
rewrites, while operation interfaces separately describe physical reads, writes, and
aliasing: <https://mlir.llvm.org/docs/Bufferization/>. LLVM ADCE likewise propagates
semantic liveness backward from live roots without claiming that this alone specifies
target storage: <https://llvm.org/doxygen/ADCE_8cpp_source.html>. Regalloc2's checker
then provides the relevant independent physical precedent by tracking which semantic
values occupy allocations at each program point:
<https://github.com/bytecodealliance/regalloc2/blob/main/src/checker.rs>.

## A-014 — Function return observability without visibility metadata

**Status:** ABI ambiguity; Stage-5E conservative policy decided.

Backward runtime demand needs to know which function results may be observed outside
the current Core call graph. Core currently has no public/private visibility,
export-root list, address-taken state, or whole-pack closed-world promise. Lowering
also publishes an entry resource and fixed parameter/result slots for every declared
function. Treating an apparently uncalled function as private would therefore invent
an optimization permission that the IR and public lowering map do not provide.

Stage 5E treats every Core function as independently externally callable and seeds
every reachable `Return` operand in every function. Internal callers may omit copies
for their own unused call-result positions, but the callee's complete result ABI and
return computation remain observable. This is conservative for a closed program and
necessary for the current open ABI. It is not a claim that every function will
actually be called at runtime.

A future visibility/export feature may make private-function return demand depend on
internal uses and may permit deleting unreachable private functions. That change must
add explicit verified visibility to Core, define whether raw Minecraft resources or
external datapacks can name private entries, and update the lowering map and datapack
ownership contract together. Call-graph absence alone is insufficient evidence.

The distinction mirrors established IRs that make this permission explicit: MLIR
symbols carry public/private/nested visibility
(<https://mlir.llvm.org/docs/SymbolsAndSymbolTables/>), while LLVM uses linkage and
visibility to distinguish module-private definitions from externally visible ones
(<https://llvm.org/docs/LangRef.html#linkage-types> and
<https://llvm.org/docs/LangRef.html#visibility-styles>). Until Core represents an
equivalent fact, whole-program demand remains rooted at every function boundary.

## A-015 — Lifetime and exclusivity of pinned ABI storage

**Status:** lowering/ABI ambiguity; Stage-5F conservative policy decided.

The word “pinned” can describe two different physical contracts. It may mean only
that an externally visible value starts in a stable, named slot, allowing the
compiler to reuse that storage after the value dies. It may instead mean that the
slot remains exclusively owned by that ABI position for the complete activation.
Core and the current lowering map do not state an ABI clobber point or a lifetime at
which an external invoker must stop observing an entry-parameter slot. The final plan
also correlates each pinned parameter home with one exact semantic value.

Stage 5F therefore chooses exclusive singleton storage:

- every entry parameter retains its existing stable value-correlated home;
- no non-entry semantic value may share that home, even when ordinary SSA liveness
  says the parameter is dead;
- function-result ABI homes remain separate, indexed slots and never participate in
  semantic-value coalescing; and
- coalescing remains function-local and applies only to ordinary same-type register
  homes introduced for demanded non-entry values.

This is not MDL undefined behavior and does not imply that reuse is inherently
unsound. It is a missing optimization permission at the ABI boundary. Reusing pinned
storage later requires an explicit activation/clobber contract, corresponding public
map semantics, and independent tests for external calls and nested internal calls;
it must not be inferred solely from local liveness.

The distinction is analogous to precolored/fixed allocation constraints in
regalloc2 and LLVM, where a stable physical location is represented separately from
ordinary coalescable virtual ranges:
<https://docs.rs/crate/regalloc2/0.13.2/source/doc/ION.md> and
<https://llvm.org/doxygen/RegisterCoalescer_8cpp_source.html>.

## A-016 — Noncanonical Boolean values supplied through the scoreboard ABI

**Status:** explicit external-ABI precondition; behavior outside the contract.

Core `Bool` has two values, while the physical scoreboard slot exposed by
`LoweringMap` can hold any signed 32-bit integer. The compiler cannot prove what an
external datapack or console wrote before directly invoking a generated entry
resource. In particular, the current `BoolNot` recipe computes `1 - operand`, so an
arbitrary non-`0`/`1` integer would not denote a Core Boolean and could propagate a
noncanonical result.

The public ABI therefore requires callers to write exactly `0` or `1` to every
parameter slot reported as `CoreType::Bool` before invocation. Supplying another
integer is a caller contract violation; the generated function makes no Core-semantic
claim for that invocation. This is not undefined behavior in a well-typed MDL
program: compiler-generated Boolean constants, comparisons, overflow flags, internal
calls, and same-type transfers preserve normalization once the entry precondition
holds. Boolean result slots are consequently `0` or `1` after every conforming
invocation.

Stage 5 keeps proof ownership explicit. Structural verification proves that Boolean
values use Boolean homes and that transfers never cross physical types. Closed scalar
recipes prove normalized Boolean production. The sparse symbolic home-content checker
proves semantic identity and read/write timing, but deliberately does not duplicate a
numeric constant-range domain merely to re-prove `0`/`1` values.

If a future runtime facade accepts untyped scoreboard inputs, it must either validate
or normalize them before entry, or change the public contract and model the chosen
coercion. It must not silently treat every nonzero score as `true` while retaining the
current arithmetic-negation recipe.
