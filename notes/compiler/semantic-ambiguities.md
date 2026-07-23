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

## A-017 — Addressability and stability of generated non-entry functions

**Status:** explicit external-ABI boundary; Stage-5G policy decided.

Minecraft datapacks have no private symbol visibility. An external datapack that
guesses a compiler-generated `__mdl` resource name can physically invoke it, even
though the compiler never published that resource. If every guessable generated
function were treated as externally stable, no non-entry block could ever be consumed
or renamed and target placement would be impossible.

The supported lowering ABI therefore consists only of each Core entry resource and
its typed parameter/result slots exposed by `LoweringMap`, together with the documented
load-tag behavior. Every mapped Core entry remains materialized. Generated non-entry
block and branch-helper resources are compiler-private implementation details: under
`MinecraftOptimizationLevel::Baseline` they may disappear, be renamed, or be replaced
by a closed control recipe. Calling one directly from outside the generated pack is a
caller contract violation and carries no stability or Core-semantic guarantee. The
`None` mode remains a byte-stable differential oracle, not an expanded public ABI.

A future feature that exports internal labels or raw callable resources must represent
that addressability explicitly, make the referenced block materialized, and update the
lowering map and verifier together. Predictability of a generated name is not such an
export. This follows the same principle as explicit symbol visibility in MLIR and
linkage in LLVM: optimization permission comes from a declared boundary, not from an
assumption that outside code will not discover a name.

- MLIR symbol visibility: <https://mlir.llvm.org/docs/SymbolsAndSymbolTables/>
- LLVM linkage and visibility: <https://llvm.org/docs/LangRef.html#linkage-types>

## A-018 — Opaque raw commands and compiler-created function boundaries

**Status:** control-flow ambiguity resolved by Stage-7B isolation.

Minecraft raw command text can contain `return` directly or beneath an `execute`
chain. `return` exits the currently executing Minecraft function, but Core models the
literal unsafe form as an ordered statement with a continuation. Emitting the raw line
directly into a compiler-created block function would therefore make behavior depend
on physical block placement: block fusion or helper selection could change which
Minecraft function the same raw line returns from. Treating `return` as impossible
would also be unjustified because the compiler deliberately does not parse unsafe
Brigadier text.

Stage 7B gives every reachable raw instruction a dedicated one-line target helper.
The containing generated block executes an ordinary `function <helper>` command.
Consequently, direct or nested `return` exits only the isolation helper, the caller
continues at the next MDL statement, and Core optimization cannot change that
boundary. The helper and call are explicit planned resources with independent
verification, provenance, cost census, and deterministic naming. The raw line still
has unknown effects, outcome, forks, context, and transitive work; isolation does not
make it safe or permit semantic inspection.

This is the same structural rule used by established compiler IRs: opaque assembly
may not secretly branch through an ordinary instruction. LLVM represents explicit
opaque branch destinations with `callbr`, while Rust inline assembly requires control
behavior such as `noreturn` to be part of the declared interface. MDL currently offers
no raw-command control-flow contract, so containment is the only conservative
statement semantics.

- LLVM inline assembly and `callbr`: <https://llvm.org/docs/LangRef.html#callbr-instruction>
- Rust inline assembly `noreturn` contract: <https://doc.rust-lang.org/reference/inline-assembly.html#options>
- Minecraft Java 1.20.3 `return run` behavior: <https://feedback.minecraft.net/hc/en-us/articles/21968446892173-Minecraft-Java-Edition-1-20-3>

## A-019 — Local executor proof versus an export's ambient executor contract

**Status:** implemented and differentially verified for the Stage 7 captured-executor
slice; future source ABI syntax remains deferred.

`ExecutionContext::function_entry()` currently marks the executor unavailable. That
is correct for lexical proof: a function body cannot manufacture a typed
`Executor<T>` capture without a modifier such as `.as(query)`. It is not equivalent
to proving that a Minecraft caller can never supply `@s`. Stage 7 also permits an
exported function to retain an inferred ambient-context requirement, so treating
“not established inside this body” as “cannot be required from the caller” would
make those two policies contradict each other.

The current `.as` slice does not expose the conflict because no typed source
operation consumes an inherited executor. Behavior inference nevertheless keeps the
domains separate: lexical context facts govern captures, while
`FunctionBehavior::required_ambient_context` is an outward source-ABI requirement.
Unsafe raw text remains `Unknown`, and `.as` can discharge its executor requirement
without pretending the other frame components are known.

The Stage 7 resolution keeps the layers distinct:

1. HIR source checking requires an exact active lexical capture for
   `Executor<T>.say`; `Unavailable` means only that the source body has no such local
   proof.
2. HIR verification replays that exact proof. HIR-to-Core then erases it rather than
   manufacturing an SSA executor value or retaining a lexical scope ID across an
   outlined-function boundary.
3. The Core semantic operation retains the instantiated executor kind and contributes
   a symbolic ambient requirement. A deterministic Core call-graph analysis propagates
   it through ordinary calls and zero-modifier scopes, while `.as(kind)` discharges a
   matching requirement.
4. Mapped unoptimized HIR/Core entry requirements are compared independently, and
   optimized Core entry requirements are published for generated functions.

This makes a nested outlined body honest without granting source code an implicit
executor variable. A future feature that lets an arbitrary datapack caller supply a
typed executor capture still needs explicit source syntax and call-site rules; Stage
7 does not infer that authority from Minecraft's possible runtime `@s` alone.

The implementation independently infers HIR behavior and unoptimized Core ambient
requirements, rejects disagreement at their boundary, retains one optimized
`CoreAmbientAnalysis`, and verifies it again during physical planning. Scale tests
cover 20,000-function chains, fanout, and recursive cycles without recursive host
traversal. The generated function ABI publishes the remaining executor requirement;
the pack-wide execution contract does not pretend it applies uniformly.

## A-020 — Plain typed `say` is smaller than native `minecraft:message`

**Status:** Stage 7 literal subset implemented and measured on Java 26.2; boundary
whitespace and selector interpolation remain intentionally unavailable rather than
implementation-defined.

Java 26.2 exposes `say <message>` through Brigadier's `minecraft:message` parser.
That native parser can recognize entity-selector syntax, while an `.mcfunction`
line ending in a backslash participates in physical line continuation. Treating an
arbitrary source string as though it were inert text would therefore give the same
characters different context reads and physical structure.

Stage 7 resolves the first slice conservatively:

1. target-independent `MessageLiteral` rejects empty text, Unicode control
   characters, and `@`; it otherwise owns and preserves the decoded source text;
2. the Java 26.2 recipe additionally rejects leading/trailing whitespace and a
   terminal backslash until their exact native/physical round trip is specified;
3. target preflight counts the native 256-code-unit bound in Java UTF-16 and checks
   the complete rendered `say ` command independently; and
4. structured target `Say` reads the executor, produces `OUTPUT`, never forks,
   continues locally, and retains native result `Exact(1)` separately from the
   source operation's discarded `Void` result.

The pinned clientless-server differential now exercises doubled internal spaces,
quotes, a nonterminal backslash, and `π` in one message and observes them exactly
under the named armor stand executor. Removing the queried entity skips the output.
A separate handwritten `execute store result ... run say ...` probe measures result
`1`; the compiler does not infer that from the Brigadier report. An automated report
audit regenerates the official 26.2 `commands.json` from the exact server bundle and
asserts `say -> message: minecraft:message`, using that evidence only for syntax.

Future structured text or selector interpolation must receive its own semantic type,
context/effect descriptor, recipe, and conformance tests. Until then these cases are
compile-time rejection, not compiler undefined behavior.

- Minecraft Java Edition 26.2 and official server bundle:
  <https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2>
- Mojang-generated command report reproduction:
  [`../mcfunction/execute-chains.md`](../mcfunction/execute-chains.md)

## A-021 — Entity movement does not update the inherited execution frame

**Status:** Stage 7.5 policy implemented and measured on Java 26.2; generalized
teleport outcome remains conservative.

Minecraft has two states that source syntax can easily conflate: the target entity's
world position and the execution frame used to interpret later relative/local
coordinates. A `teleport` command mutates the entity, but it does not retroactively
change the position, rotation, dimension, or anchor inherited by the next command in
the same function invocation. Likewise, `execute as <entity>` changes the executor
without copying that entity's spatial frame; `execute at @s` is required when the
entity's position/dimension/rotation should become current.

Stage 7.5 therefore gives the two source methods intentionally different meanings:

```text
executor.teleport(~, ~1, ~) // relative to the current execution frame
executor.move_by(0, 1, 0)   // relative to the receiver via `execute at @s`
```

The clientless Java 26.2 probes establish the following facts:

1. `as` preserves the incoming spatial frame, while `at` copies the selected
   entity's dimension, position, and rotation;
2. sequential modifiers consume the frame produced by the preceding modifier;
3. local coordinates respond to current rotation and feet/eyes anchor;
4. `in` transforms the current position with the documented Overworld/Nether scale,
   so exchanging `in` and `positioned` is observable;
5. a successful one-target teleport reports success/result `1`, while an empty
   target reports `0`; and
6. commands after teleport continue to interpret coordinates in the original
   inherited execution frame.

The semantic operation discards Minecraft's native result and returns source
`Void`. Target analysis still classifies the general teleport native outcome as
`Unknown`: the two measured cases do not prove every target count, collision,
dimension, loading, or failure condition. Optimizers may not use the measurements to
invent a universally exact result or to rewrite frame-relative teleport into
receiver-relative movement.

Evidence:

- [`../../crates/mdl-test/tests/spatial_command_semantics.rs`](../../crates/mdl-test/tests/spatial_command_semantics.rs)
- [`../../crates/mdl-test/tests/stage75_server.rs`](../../crates/mdl-test/tests/stage75_server.rs)
- [`../mcfunction/coordinate-frames.md`](../mcfunction/coordinate-frames.md)

## A-022 — Synchronous activation order and abnormal recursive-frame residue

**Status:** Java 26.2 behavior measured; Stage 8 explicit-recovery policy implemented.

An `execute as` redirect may invoke one outlined body many times, but runtime
multiplicity does not imply simultaneously live activations. A pinned clientless
Java 26.2 fixture ran three armor-stand contexts through a nested function while
sharing one score. Each child observed and completed its full before/nested/after
sequence before the next child began. Selector iteration order remains unspecified;
complete child unwind is the property the compiler relies on. Consequently bounded
many-context source scopes may reuse static score homes when no value escapes the
child activation.

Recursive calls are genuinely reentrant and use a compiler-private command-storage
tail list. Java 26.2 accepts append, `[-1]`/`[-2]` reads and writes, typed score/NBT
bridges, and tail removal with the expected synchronous behavior. Compiler-generated
direct and mutual recursion passed under all four Core/Minecraft optimization
policies, including a caller value live across the recursive edge.

Command-sequence interruption is not stack unwinding. With the sequence limit
reduced after a frame append, Java aborted the root before the normal pop and the
frame remained observable. Stage 8 therefore makes no transactional claim: world
writes and private frames may remain after abnormal termination. The generated
`<namespace>:__mdl/load` function is the explicit recovery entry; it replaces the
private frame list with `[]` before performing ordinary initialization. External
callers must not invoke another MDL export while the runtime is known to be poisoned;
they must invoke the load/recovery entry or reload the datapack first. Internal calls
never use that recovery entry.

Evidence:

- [`../../crates/mdl-test/tests/stage8_activation_contract.rs`](../../crates/mdl-test/tests/stage8_activation_contract.rs)
- [`../../crates/mdl-test/tests/stage8_compiler_server.rs`](../../crates/mdl-test/tests/stage8_compiler_server.rs)
- [`stage-8/8-0-contract-and-evidence.md`](stage-8/8-0-contract-and-evidence.md)

## A-023 — Command channel absence is distinct from numeric zero

**Status:** Java 26.2 behavior measured; PS-1 typed observation policy implemented.

Minecraft does not guarantee that an `execute store success` or `execute store
result` destination is written. A function that completes without producing a
command result leaves both attached destinations unchanged. An `execute as` redirect
with zero child contexts likewise performs no store callback. These cases are
observably different from ordinary command failure, which writes success `0` and
result `0`.

Java 26.2 also reports a function executing `return 0` as success `1`, result `0`.
The numeric return value therefore cannot stand in for command success. The compiler
and semantic harness must retain channel availability, channel value, and outer
continuation separately. Until a structured operation has a precise native outcome
contract, target analysis remains conservative rather than inferring failure from a
missing context or zero result.

The complete measured matrix and reproduction are recorded in
[`../mcfunction/command-outcomes.md`](../mcfunction/command-outcomes.md).

## A-024 — Phase-1 arithmetic overflow and loop-control boundaries

**Status:** Source semantics frozen and implemented in PS-2A.

Plain signed arithmetic must not inherit whichever overflow behavior happens to be
convenient in Rust, Core, or Minecraft scoreboards. Phase 1 therefore exposes only
explicit wrapping `Int32` addition and subtraction as `+%` and `-%`. Plain `+` and
`-` remain reserved, and division/remainder remain absent until their zero and
negative-edge behavior is separately frozen. Brainfuck bytes are normalized
`Int32` values in `0..=255`; library boundary branches implement byte increment and
decrement without assuming a native `UInt8` or target remainder rule.

`while` is a pre-test loop. Its condition runs once per attempted iteration and
source expressions evaluate left-to-right. `break` and `continue` bind to the
innermost lexical loop and cannot cross an outlined `run` boundary. A loop may run
zero times, so assignments first made only in its body do not become definitely
assigned after it. Runtime termination does not imply that static target analysis
can prove a finite command bound. The PS-2 fuel client supplies deterministic
semantic termination, while the current target analysis honestly retains
`NoFiniteBoundProven` for its data-dependent CFG cycle.

The implementation and evidence are indexed in
[`pre-scheduler/ps-2/arithmetic-and-control-flow.md`](pre-scheduler/ps-2/arithmetic-and-control-flow.md).

## A-025 — Runtime string units are target-aligned UTF-16 in Phase 1

**Status:** implementation-defined Phase-1 contract; frozen Unicode-scalar proposal revised.

Minecraft Java 26.2 `data modify ... string` indexes Java UTF-16 code units. The
first implementation lowers `String.length`, `ends_with_ascii`, and
`without_last_unit` directly through that representation, so claiming Unicode-scalar
length or consumption would be false. Phase-1 `String` therefore has immutable value
semantics over Java UTF-16 units. A supplementary scalar contributes two units.

Brainfuck recognition remains correct because every opcode is one ASCII unit. A
non-ASCII supplementary character is consumed as two ignored units; it never becomes
command syntax. A future Unicode-scalar API must validate/combine surrogate pairs or
use an explicitly converted representation. It cannot silently change the meaning
of the existing target-aligned primitive.

Evidence is recorded in
[`../mcfunction/written-books-26.2.md`](../mcfunction/written-books-26.2.md) and
`crates/mdl-compiler/tests/ps2_runtime_strings.rs`.

## A-026 — Written-book raw content is a typed partial conversion

**Status:** supported subset explicit; other text-component shapes unsupported.

The Java 26.2 entity/item path is target-version-specific and runtime-dependent.
The first intrinsic accepts a current inventory-capable executor and a static page
index, then reads
`equipment.mainhand.components."minecraft:written_book_content".pages[i].raw`.
Vanilla normalizes a simple literal raw component to an NBT string. Styled compound
or list components and dynamic text kinds are not equivalent to that string path.

Phase 1 therefore defines the intrinsic as “literal page or empty,” not as arbitrary
client-rendered text. Missing equipment, wrong items, absent pages, and unsupported
non-string raw content return empty after an explicit fallback initialization. This
is defined behavior, not leaked command failure. Flattening styled/nested literal
spans and returning distinct absence/unsupported variants remain future typed API
work; the compiler must not guess locale-, entity-, score-, or NBT-dependent client
rendering.

PS-3 deliberately maps every empty conversion to its adapter-level `NO_PROGRAM`
result. It cannot distinguish a missing holder slot, wrong item, absent page, empty
literal page, or unsupported raw-component shape because the accepted public
operation intentionally erases that distinction. A future richer API must return a
typed variant; inferring the cause from an empty string would be incorrect.

## A-027 — Brainfuck dispatch fuel excludes bracket-search work

**Status:** application semantics frozen and differentially implemented in PS-3.

PS-3 consumes one semantic fuel unit after removing one normalized opcode from the
future cursor and before applying that opcode. Jumping from a zero `[` to its
matching `]`, or from a nonzero `]` back to its matching `[`, moves cursor elements
but does not dispatch those scanned instructions and consumes no additional
Brainfuck fuel. A nonzero `]` requeues itself after the body so later iterations
dispatch the close again; the matching open is not re-dispatched.

This makes output and fuel independent of a particular jump-table optimization, but
semantic fuel is not a direct bound on emitted Minecraft commands. Search cost is
also bounded by the admitted normalized program length for a selected concrete run,
yet current target CFG analysis does not combine that value bound with runtime fuel
and correctly retains `NoFiniteBoundProven(PositiveCycle)`. Stage 9 must treat an
in-progress search as an atomic bounded region or make its cursor suspendable.

Evidence:

- [`../../tests/programs/brainfuck/cases.json`](../../tests/programs/brainfuck/cases.json)
- [`../../crates/mdl-test/tests/ps3_brainfuck.rs`](../../crates/mdl-test/tests/ps3_brainfuck.rs)

## A-028 — Dimension reconstruction across a `schedule`/`#minecraft:tick` boundary is
unmeasured, not merely unknown

**Status:** compile-time unknown; measurement attempted and invalidated 2026-07-23.

Stage 9's cut-legality rule (domain 4) requires knowing what execution context —
including dimension — a scheduled or tick-tag-fired function actually receives.
Executor loss and the ambient default *position* (the world spawn point, not the
scheduling site) are confirmed
([`stage-9/9-0-contracts-and-evidence.md`](stage-9/9-0-contracts-and-evidence.md),
M14), but the dimension component was not successfully measured. The attempted test
(schedule from `execute in minecraft:the_nether`, then probe which dimension a
`~ ~ ~`-summoned marker lands in) produced a self-contradictory result: both an
overworld-scoped and a nether-scoped presence check matched the same,
definitely-overworld-resident test entity. A follow-up diagnostic
(`recon_dimension_selector_scoping` in the same test file) found that
`execute in <dim> as/if entity @e[...]` did not reliably filter by dimension under
this harness's test conditions even after fixing an initial modifier-ordering bug
(`as @e[...] in <dim>` evaluates the selector before `in` takes effect, which is a
real, separate finding worth remembering on its own).

The compiler must not assume "self-rooting reduces to overworld" (or any other
specific dimension) for 9B's scheduled/tick-tag entries until a redesigned
measurement — forceloading the relevant chunk in *every* candidate dimension before
testing, and verifying `execute in <dim> as @e[...]`'s filtering semantics in
isolation first — produces a trustworthy answer. Until then, dimension context
should be treated as `ContextRequirement::Unknown` rather than assumed `None`
wherever the generated-entry-requirement check (`AmbientContextRequirements`,
`crates/mdl-compiler/src/ir/semantic/behavior.rs`) is applied to a scheduled entry's
dimension component specifically.

## A-029 — Player-presence selector leakage into a scheduled/tick-tag function is
unmeasured

**Status:** compile-time unknown; not yet attempted.

Whether a fired `schedule`/`#minecraft:tick` function's default entity-selector
state (e.g. `@e[limit=1,sort=nearest]` with no other constraint) can be influenced
by, or leak information from, a connected player being nearby versus absent was
planned as measurement M16 in
[`stage-9/9-0-contracts-and-evidence.md`](stage-9/9-0-contracts-and-evidence.md)
but was not run in the 2026-07-23 pass — it requires a real Azalea client
(`crates/mdl-test-bot::BotHandle`) standing alongside the frozen-tick harness, which
was out of scope for that pass's time budget. Executor loss itself (M14) is already
confirmed without a connected player; this entry covers only the narrower
"does a *present* player change anything" question. Treat as unmeasured, not as
implicitly "no" by absence of a demonstrated leak.

## A-030 — `append`-mode reload duplication hazard is real datapack practice but
unreproduced

**Status:** unspecified behavior; negative measurement recorded 2026-07-23, not
otherwise explained.

`spawn-animations` (see
[`../functionality-keystones/datapacks/spawn-animations/NOTES.md`](../functionality-keystones/datapacks/spawn-animations/NOTES.md))
defends its load-time self-reschedule with an explicit `schedule clear` before a
fresh `schedule function ... 1s`, citing the risk that `/reload` would otherwise
stack a duplicate chain. Stage 9.0 measured `replace`-based re-arm (safe, confirmed,
M12) and `append`-based re-arm at both a matched delay (safe, M13a — consistent with
M7's finding that `append` at an already-pending target tick is a no-op) and a
deliberately *mismatched* delay modeled on `spawn-animations`'s own shape (5t
internal chain, 3t load-time re-arm, M13b) intended to reproduce the hazard `append`
+ a different target tick should, per M8, be able to create. **It did not
reproduce**: `loop_count` advanced by exactly the single-chain expected amount with
no excess.

This is recorded as an honest negative result, not resolved into either "the hazard
does not exist" or "this test failed to trigger it for an identifiable reason." Does
not block any current Stage 9 decision — MDL's own lowering uses `replace`
unconditionally per the plan's frozen decision, sidestepping the question — but a
future session investigating `append`-mode scheduling more deeply (or citing
`spawn-animations` as evidence of a specific hazard) should re-run and extend this
measurement rather than assume either this note's negative result or the datapack
author's defensive practice settles the question.

Evidence:

- [`../../crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs`](../../crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs)
  (`group4_reload_rearm_hazard`, `group5_execution_context_loss`,
  `recon_dimension_selector_scoping`)
- [`stage-9/9-0-contracts-and-evidence.md`](stage-9/9-0-contracts-and-evidence.md)
