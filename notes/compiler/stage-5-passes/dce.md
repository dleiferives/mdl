# Dead Core Instruction Elimination Pass

Status: **Implemented, pipeline-integrated, and fast-gated**

## Purpose and exact boundary

Remove trivially dead attached Core instructions after SCCP and after the final
fusion/CSE cleanup. This is instruction DCE over the existing SSA use graph. It is
not aggressive DCE: Stage 5 does not rewrite control flow, delete block parameters,
solve demand through edge-argument cycles, or remove loop-carried dead cycles.

The pass is deliberately smaller than GCC's control-dependence DCE, GHC's
SCC-sensitive recursive-binding elimination, or Cranelift's demand-driven e-graph
elaboration. Those approaches are useful evidence for later work, but importing their
machinery into the first optimizer would duplicate SCCP/reachability and Stage 5E
runtime-demand responsibilities.

## Semantic erasure contract

Core owns one exhaustive semantic query:

```text
CoreOp::is_trivially_discardable()
    = effects() == EffectClass::Pure
   && speculation() == Speculation::Always
```

Both the pass and `erase_discardable_inst_set` call this query; neither copies an
operation allowlist. The conjunction is intentionally conservative:

- `Pure` excludes calls and any observable Core effect.
- `Always` excludes a future pure operation that can trap, diverge, exhibit undefined
  behavior, or transfer control non-locally. Removing an executed operation must not
  remove that behavior merely because its SSA results are unused.
- unknown semantic classifications are non-discardable.

This matches MLIR's useful separation between memory/resource effects and
speculatability, while adopting the stronger single `Pure + Always` gate for MDL's
small Core. Zig makes the analogous policy distinction between an unused AIR result
and an instruction that `mustLower`; LLVM centralizes the corresponding target-aware
question in `isInstructionTriviallyDead`.

An attached instruction is selected only when it is trivially discardable and every
result has no remaining attached use outside the selected set. The all-results rule is
one instruction-level contract: a partially unused `I32AddOverflowing` cannot be
erased. A future zero-result `Pure + Always` instruction is vacuously eligible; a
zero-result call is not.

Attached branch conditions, edge arguments, returns, and operands of non-selected
instructions are ordinary uses and therefore roots implicitly. Calls always remain,
and their operands keep producers live. Detached raw instructions are historical data,
not executable users or candidates.

## Discovery algorithm

Discovery is read-only and nonrecursive:

1. Require the runner's verified pass boundary, including the Stage 5B invariant that
   every allocated `InstId` occurs in at most one raw block instruction container.
2. Before dense scratch allocation, compare allocated value/instruction cardinalities
   with the typed DCE scratch-entry limit. If the configured optional limit is too
   small, return `StoppedAtLimit` unchanged. Invalid entity arithmetic is a compilation
   failure, not an optimization fallback.
3. Allocate one dense `remaining_uses[value]` counter table and populate it with one
   direct attached-layout scan. Count every operand occurrence separately and include
   branch conditions, edge arguments, and returns. DCE never needs complete `UseSite`
   records, so constructing a `Vec<Vec<UseSite>>` only to copy its lengths would add
   avoidable allocations and memory traffic. Finish the immutable count scan before
   opening the mutable editor.
4. Scan attached instructions in authoritative block/instruction order. Enqueue each
   discardable instruction for which every result count is zero into a deterministic
   `VecDeque`. Maintain ID-indexed `queued` and `selected` bits.
5. Pop one candidate. Charge one candidate visit *before* changing virtual state. A
   visit is atomic: once admitted, select the complete instruction and process every
   operand occurrence without consulting the limit again.
6. For every operand occurrence, checked-decrement its remaining count. After all
   decrements, inspect instruction producers touched by those operands in first-operand
   order. Enqueue an unqueued discardable producer only when every one of its result
   counts is now zero. Block parameters have no instruction producer and stop this
   backward walk.
7. Continue to worklist exhaustion or stop before the next whole candidate visit. Call
   `erase_discardable_inst_set` once with the selected set, including when completion is
   `StoppedAtLimit` and the set is nonempty.

The zero-count proof is simple: `remaining_uses` begins as the number of all attached
uses, and the algorithm decrements it only for operand occurrences in already-selected
instructions. Therefore a selected producer with zero remaining uses has no attached
user outside the selected set. Terminator uses are never decremented. This also proves
that a limit-stopped selected prefix is safe to apply.

Do not stop halfway through an instruction's duplicate operands, mark an instruction
selected before budget admission, mutate Core during discovery, rebuild `UseIndex` per
deletion, recurse through producer chains, or decrement once per distinct operand
value. Each of those variants either breaks the proof or the complexity bound.

## Checked batch application

`erase_discardable_inst_set` treats the selected IDs as an untrusted proposed edit. In
one complete preflight it must:

- reject invalid, detached, or multiply-owned selected instructions;
- recheck `is_trivially_discardable()` for every selected instruction;
- independently scan current attached operands and terminators, rejecting any use of a
  selected result except an instruction-operand use whose user is also selected; and
- finish all checked sizes and scratch construction before mutation.

The independent scan must be linear over current attached layout. It must not perform
one whole-body scan per selected result, and it need not allocate a second `UseIndex`.
This editor-side proof prevents a stale or corrupted DCE plan from turning into a
partial mutation.

After preflight, retain only unselected IDs in each attached block's instruction vector
in authoritative order. Do not erase arena entities, clear detached `InstData`, rewrite
operands, or touch detached block containers: stable IDs and their provenance remain
available to the malformed-safe debug dumper. A batch failure mutates nothing. A later
runner verification failure drops the owned compilation unit rather than exposing the
partially optimized program.

The legacy `erase_pure_inst` is not the Stage 5 implementation: it checks only
`EffectClass::Pure`, rebuilds facts per call, and removes one instruction at a time.

## Limits, results, and complexity

There are two distinct bounded outcomes:

- A configured DCE scratch-entry limit that is exceeded before analysis yields
  `StoppedAtLimit` with no Core change and no large dense allocation.
- Candidate-visit exhaustion stops before the next atomic visit and applies the
  already-proven selected subset. `changed` is true exactly when that subset is
  nonempty.

Representable entity-capacity or invariant failures are compilation failures. System
allocator exhaustion follows Rust's process allocation policy. Statistics saturate as
specified by the framework and never fail a valid optimization.

On complete worklist exhaustion, DCE reaches a fixed point for this pass's
instruction-only rule; an immediate second complete run changes nothing. A
limit-stopped run does not promise idempotence because another invocation may spend
fresh fuel.

The CFG, block order, dominance, reachability, signatures, parameters, and terminators
are preserved. Placement and all use/liveness facts are invalidated. Runtime-demand
analysis must run later on the resulting Core, not reuse a pre-DCE inventory.

Time is `O(attached instructions + attached operand/terminator uses + selected
instructions)`. Current scratch is
`O(allocated values + allocated instructions)`, not merely attached entities: use
counts and ID bitsets are dense over stable allocated IDs, including historical
detached entities. The pre-allocation scratch limit makes that phase-1
tradeoff explicit. A sparse/compacting identity redesign is not part of Stage 5.

## Pipeline interactions and deliberate non-goals

- SCCP first folds branches, detaches unreachable blocks, and replaces constants; DCE
  then removes newly unused scalar chains without consuming incomplete SCCP facts.
- Fusion and CSE can expose more dead producers, so the bounded final cleanup invokes
  the same DCE implementation once when an enabling pass changed Core.
- Canonicalization and CSE use the same checked erasure batch and semantic query; they
  do not grow separate notions of discardability.
- Stage 5E runtime demand may avoid physicalizing a loop-carried dead value cycle that
  this Core pass retains. Physical pruning does not retroactively justify mutating
  block-parameter contracts here.
- Removing cyclic instruction/parameter groups requires dependency SCCs or a
  mark-from-roots formulation that understands edge arguments. GHC demonstrates the
  SCC route and GCC demonstrates the control-dependence route; both are deferred until
  a measured case justifies an explicit pass.
- Debug-only uses do not exist in current Core. If they are added, their droppable or
  retaining semantics must be represented explicitly rather than silently omitted
  from `UseIndex`; LLVM salvages debug records and rustc handles debug-info locals
  deliberately.

## Tests

Semantic and editor contract:

- Exhaustive tests for `CoreOp::is_trivially_discardable()`: every current scalar op is
  true and every call is false; the query remains the only pass/editor gate.
- Dead leaves, complete and partial multi-result use, duplicate operands such as
  `x + x`, long chains, diamonds, and shared producers.
- Calls with unused results remain and keep every argument producer live.
- Branch-condition, both branch-edge argument positions, jump arguments, and every
  returned result keep their producers live.
- Corrupted sets containing a call, detached ID, invalid ID, or an instruction with an
  external instruction/terminator use are rejected atomically.

Stable identity and control flow:

- Detached users do not keep an attached producer live; detached instructions are not
  selected; attached uses of detached definitions fail verification before DCE.
- An attached/detached raw-container ownership collision is rejected at the verified
  boundary. Successful DCE changes only attached instruction vectors and retains raw
  instruction data and origins.
- A loop-carried block-parameter cycle demonstrates the documented non-transform.
  CFG, block parameters, edge arguments, and block order remain byte-for-byte equal.

Limits, determinism, and scale:

- Zero, one, exact, and mid-chain candidate limits apply only whole-instruction
  transitions; each partial set passes the editor's independent closure scan.
- A scratch limit below dense allocated-ID cardinality returns unchanged before
  allocation, including a body with many historical detached values.
- FIFO seeding and first-operand producer order produce the same selected IDs, remarks,
  and dump across repeated runs.
- A complete run is idempotent. On small generated verified functions, compare the
  selected result with an independent naive oracle that repeatedly rescans for one
  unused discardable instruction, then verify the optimized body.
- A 20,000-instruction chain completes without host recursion, performs one use-count
  scan, invokes one erasure batch, and stays within linear scan counters.
- Pipeline fixtures cover SCCP-created dead chains and fusion/CSE-created dead
  producers, including the no-enabling-change skip of final cleanup.

## Cross-compiler conclusions

| Compiler | Relevant design | Stage 5 conclusion |
| --- | --- | --- |
| LLVM | Simple DCE erases an unused trivially-dead instruction, nulls each operand use, and queues newly dead producers; it preserves CFG analyses. | Use the same backward producer worklist, but discover virtually and apply one checked batch because MDL retains stable entities and has an atomic editor boundary. |
| MLIR | Effects and speculatability are separate interfaces; `Pure` combines speculatability with memory-effect freedom, and deadness considers the complete operation result use-set. | Keep one exhaustive `Pure + Always` Core query and one all-results instruction contract. Do not import MLIR's region/effect-resource generality before Core needs it. |
| rustc | MIR dead-store elimination computes liveness first, accumulates patches, then mutates; its source explicitly protects idempotence and interactions with destination propagation and debug info. | Preserve analysis-before-mutation, test complete-run idempotence and neighboring-pass interactions, and make future debug uses explicit. MIR's place/borrow analysis is otherwise unnecessary for SSA Core. |
| Zig | AIR liveness marks unused instructions, skips their operand liveness only when the opcode need not `mustLower`, and has an independent liveness verifier. | Keep dead-result status separate from semantic necessity, and independently verify the editor set instead of trusting the chooser's counts. |
| Cranelift | The e-graph pass removes pure nodes from the side-effecting layout skeleton and elaborates only values demanded by roots, using explicit traversal stacks. | Stage 5's simple DCE remains a small pass; broader demand-driven omission belongs to the later Minecraft planner and all deep walks remain iterative. |
| GHC | Occurrence analysis performs dependency SCC analysis and drops an entire recursive SCC when no binder is demanded by the body. Haskell's lazy binding semantics differ from strict Core evaluation. | Do not claim loop-cycle deletion from leaf DCE. Add SCC-aware Core DCE only with an explicit strictness/effect proof and measured need. |
| GCC | Tree-SSA distinguishes conservative simple DCE from aggressive control-dependence DCE and treats throwing/side-effecting statements as necessary. | Preserve all terminators and CFG in Stage 5; control-dependence and loop removal are a separately designed future pass. |

## Primary references

- LLVM simple DCE implementation:
  <https://llvm.org/doxygen/DCE_8cpp_source.html>
- LLVM trivially-dead instruction utilities:
  <https://llvm.org/doxygen/Transforms_2Utils_2Local_8h_source.html>
- MLIR effects/speculation rationale:
  <https://mlir.llvm.org/docs/Rationale/SideEffectsAndSpeculation/>
- MLIR deadness, effects, and speculatability implementation:
  <https://mlir.llvm.org/doxygen/SideEffectInterfaces_8cpp_source.html>
- rustc MIR dead-store elimination and its pass-interaction contract:
  <https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_mir_transform/dead_store_elimination.rs.html>
- Zig AIR liveness and its unused-versus-`mustLower` rule:
  <https://github.com/ziglang/zig/blob/master/src/Air/Liveness.zig>
- Zig's independent AIR liveness verifier:
  <https://github.com/ziglang/zig/blob/master/src/Air/Liveness/Verify.zig>
- Cranelift e-graph skeleton and demand-driven elaboration:
  <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/codegen/src/egraph/mod.rs>
- GHC occurrence analysis, dependency SCCs, and dead recursive groups:
  <https://gitlab.haskell.org/ghc/ghc/-/blob/master/compiler/GHC/Core/Opt/OccurAnal.hs>
- GCC tree-SSA simple/aggressive DCE:
  <https://gcc.gnu.org/git/?p=gcc.git;a=blob;f=gcc/tree-ssa-dce.cc;hb=HEAD>
