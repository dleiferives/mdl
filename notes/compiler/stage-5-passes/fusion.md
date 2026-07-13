# Straight-Line Jump-Region Fusion Pass

Status: **Implemented, pipeline-integrated, and fast differential-gated**

## Purpose and boundary

Fuse maximal chains of Core blocks separated only by unconditional jumps. Removing a
boundary also removes its block-argument transfers and gives the following CSE pass
canonical operands in one instruction sequence. This is deliberately not jump
threading, branch folding, tail duplication, code motion, block reordering, loop
rotation, or a general `SimplifyCFG` implementation.

The pass is target-independent: it does not consult Minecraft command costs or choose
whether one particular boundary is profitable. Every eligible boundary is redundant
in Core and is removed.

## Exact eligibility

Derive reachability and incoming **edge** counts from one verified-body snapshot. An
edge `A -> B` is a fusion edge exactly when:

- `A` and `B` are attached and entry-reachable;
- `A` ends in `Jump(B, arguments)`;
- that jump is the only attached incoming edge to `B`;
- `A != B`; and
- `B` is not the entry block.

Count edges, not distinct predecessor blocks. A branch with two arms targeting `B`
contributes two incoming edges even when both arms originate in the same block. This
matches MLIR's distinction between a single predecessor edge and a unique predecessor
block. Core's current `ControlFlowGraph::predecessors` already retains duplicate block
IDs per edge, so eligibility may use `predecessors(B).len() == 1` together with the
recorded predecessor being `A`; do not deduplicate that slice.

Arity, types, attached definitions, edge-use dominance, and the ban on targeting entry
are already properties of verified Core, but the fusion batch rechecks the exact edge
shape it consumes. Attached unreachable predecessors count and conservatively prevent
fusion. Unlike rustc's combined CFG cleanup, this pass does not retarget or delete such
predecessors itself.

Two-arm same-target branches are never accepted directly. The earlier canonicalizer
may turn one into a jump only when both arms also have identical argument lists; fusion
then sees one real edge in its fresh snapshot.

## Why the substitution is dominance-safe

No arbitrary `ValueId -> ValueId` map is accepted from the pass. For each fused edge,
the editor derives the destination-parameter replacements from that edge's arguments.
The following local proof replaces a potentially expensive per-use dominance audit:

1. verified SSA proves every argument is defined before the `A` terminator;
2. because reachable `B` has exactly one attached incoming edge and is not entry, `A`
   dominates `B`;
3. verified SSA proves `B` dominates every reachable attached use of each `B`
   parameter; attached unreachable uses have no execution/dominance obligation but
   are still rewritten so they do not reference a newly detached definition; and
4. after `B`'s instructions move behind `A`'s instructions, the argument definition
   still precedes those formerly-in-`B` uses.

The proof composes inductively along a chain. A later edge may pass an instruction
result from an earlier consumed block; that instruction is moved before the later
block's instructions. Uses outside the region remain dominated as well. Consequently
fusion does not construct a `DominatorTree` in production. Corruption tests and the
post-pass Core verifier still test the conclusion.

## Region discovery

Construct one opaque `PreparedJumpFusion<'editor>` under an exclusive mutable borrow
of the `FunctionEditor`. It owns the reachability, incoming-edge, eligible-successor,
claimed, and raw-ownership scratch tables; exposes discovery and then batch application;
and prevents another editor mutation from making its facts stale. This is a
consumer-specific prepared edit, not a reusable public analysis or a general
transaction. It also avoids rebuilding CFG facts in the batch solely to distrust the
pass that just computed them.

Build the fusion-edge relation once. It has at most one outgoing edge per block, and
every destination has exactly one incoming edge. A region head is the source of a
fusion edge that is not itself the destination of another fusion edge. Walk region
heads in authoritative `block_order`, following fusion edges iteratively and claiming
each consumed block once. Never use host-language recursion.

This head-first rule matters when layout order differs from CFG order. Starting a
forward walk at every unclaimed layout block can claim the middle of a chain first and
silently produce non-maximal regions.

Eligible-edge cycles cannot occur in a valid reachable Core body: a path from entry
would add another incoming edge to the first cycle block, while an entry block inside
the cycle would require an edge targeting entry, which Core forbids. A final terminator
may nevertheless target the surviving region head; for example, fusing a loop header's
single-entry body can produce a valid self-loop. If a complete, non-limited discovery
leaves a fusion edge unclaimed, treat that as stale facts or an internal invariant
failure rather than inventing a cycle policy.

The pass gives the editor only minimal authoritative input:

```text
JumpRegionEdit {
  blocks: [head, consumed_1, ..., tail] // at least two
}
```

Do not copy instruction lists, terminators, edge arguments, parameter maps, or incoming
counts into the edit. The editor derives and preflights those from the body, avoiding a
second stale representation. Detailed eliminated-edge remarks, when requested, may
capture the jump origins during discovery; normal execution retains only fixed
counters.

## Parameter composition

Process a region from head to tail with a directional replacement map, not union-find:

1. resolve every argument of the next edge through replacements established by
   earlier edges;
2. resolve the complete argument list before inserting any parameter mappings for that
   destination, preserving simultaneous block-argument semantics; and
3. map destination parameters to the resolved arguments, with iterative path
   compression.

Reject a conflicting mapping or replacement cycle as an internal invariant failure.
Build one final map for every disjoint region and rewrite all attached instruction and
terminator uses in one body scan. This includes uses in surviving descendants outside
the fused region. Detached historical code is not executable and is not rewritten.
Allocated parameter values and their bidirectional `ValueDef::BlockParam` records stay
intact in the detached blocks.

## Atomic batch application

The prepared edit's `fuse_jump_regions_batch` performs all preflight before semantic
mutation. It verifies:

- every edit contains at least two distinct, allocated, attached, reachable blocks;
- no block occurs twice within or across edits, and no consumed block is entry;
- every internal edge is the exact eligible jump from the fact snapshot;
- the first block has no eligible incoming edge and the tail has no eligible outgoing
  edge, so each submitted edit is maximal;
- all destination parameter lists match the edge arguments in arity and type;
- every final-tail successor will survive the whole batch (a tail may target its own
  head, but never a consumed block);
- checked instruction-count/capacity arithmetic succeeds; and
- each allocated `InstId` occurs in at most one raw block instruction vector, including
  detached blocks.

The raw-container check is separate from executable placement. It remains a batch
precondition even after the Core verifier gains the same invariant, so the ownership
assumption is local and explicit.

After preflight, reserve every head instruction vector once, then perform only
infallible structural moves in stable region-head order:

1. rewrite the composed parameter replacements in one attached-body scan;
2. `take` and discard the head's old jump terminator;
3. for each consumed block in chain order, `take` its instruction vector and append
   those existing IDs to the head;
4. `take` every consumed terminator, discarding intermediate jumps and retaining the
   tail terminator;
5. install that exact tail terminator on the head; and
6. remove all consumed blocks from `block_order` in one retain scan, preserving the
   relative order of every survivor.

Do not clone operations or terminators, and do not implement a maximal region as
repeated single-block edits. Every consumed block remains allocated with its original
origin and parameter records, but has an empty instruction vector and no terminator.
The editor is not a rollback transaction: checked shape/size failure occurs before the
first rewrite, and an unexpected invariant failure after mutation aborts the owned
compilation unit. Head capacity overflow is ruled out by checked arithmetic before the
ordinary `reserve_exact` calls; those calls do not claim recoverable allocator failure.
System allocator exhaustion follows Rust's process allocation policy, as it does for
the earlier fact-table allocations.

## Provenance and analysis invalidation

- The head keeps its block identity and block origin.
- Moved instructions keep their `InstId`, result identities, and instruction origins.
- The moved final terminator keeps its exact origin.
- Consumed blocks and parameters keep their allocated identities and origins in
  malformed-safe/debug dumps.
- Eliminated jump origins do not need a new synthetic IR owner; a capped detailed
  remark may report them when explicitly enabled.

Fusion invalidates placement, uses, reachability, CFG, and dominance facts. No such
fact crosses the batch boundary. CSE rebuilds what it needs from the fused body.

## Limits and complexity

Use one typed `FusionFactTableEntryLimit` for dense scratch indexed by stable allocated
block/instruction IDs, and one `FusionBlockVisitLimit` for region discovery. Compute
required table lengths and bytes with checked arithmetic before allocation. Fact
admission is deliberately two-stage because the exact flattened substitution-slot
count depends on the CFG facts being admitted:

1. preflight and admit six allocated-block tables plus the allocated-instruction raw
   ownership table;
2. classify reachability, exact incoming edges, and eligible destinations, then count
   exactly one flattened mapping slot per parameter of an eligible destination; and
3. preflight the complete base-plus-mapping requirement before allocating that mapping
   table.

If the configured limit is below the classification base, return that exact base as
the minimum requirement; the full eligible-parameter total is intentionally not
derived by allocating past the limit. If the base fits but the complete requirement
does not, return the exact complete requirement. Both cases are unchanged typed
incomplete outcomes, and no region has yet been discovered or applied. If the visit budget is
exhausted while walking a region, discard that unfinished region, stop discovery, and
apply only earlier complete maximal regions. Do not continue at a suffix of the same
chain, and do not add a separate per-region-size knob in Stage 5.

With no limit, each attached block and edge is classified once, each region block is
walked once, path compression visits each parameter mapping a bounded number of times,
and fact construction scans every allocated raw instruction membership once.
Application additionally rechecks each instruction membership in the selected regions
against that frozen owner table before committing. The target is linear in allocated
raw membership plus selected-region membership, attached CFG, operands, terminator
arguments, and block parameters. Tests instrument fact builds, region visits, both raw
ownership traversals, replacement visits, and the single layout retain; wall clock
time is not the contract.

## Pipeline interaction

- SCCP/unreachable cleanup runs first, making conservative attached-edge counting
  effective in normal baseline compilation.
- DCE runs before fusion, so dead instructions are not moved.
- Fusion precedes CSE because eliminating block parameters can reveal identical
  canonical operands.
- If fusion or CSE changes Core, the pipeline performs its one bounded final
  canonicalization/DCE cleanup. That cleanup may expose another fusion opportunity,
  but Stage 5 deliberately does not iterate fusion to a fixed point.

## Tests

The current standalone suite covers the transformation and editor boundary directly:

- adversarial CFG/layout order, simultaneous multi-edge parameter composition, moved
  results, stable IDs/origins, detached consumed blocks, and exact structural counters;
- duplicate same-target edges, attached-unreachable incoming edges, pre-existing self
  edges, and a fused final backedge to the surviving head;
- exact first/second-stage fact limits, a complete multi-region prefix, a visit limit
  inside a region that leaves the whole body unchanged, and the exact admitted
  boundary;
- 1,000 detached blocks/instructions/values proving fact admission and raw ownership
  use allocated history rather than attached counts;
- rewriting consumed parameters in both surviving branch descendants;
- two disjoint regions where one fused tail branches to the other surviving region
  head, proving whole-batch successor validation distinguishes heads from consumed
  blocks;
- duplicate raw ownership, non-maximal/overlapping edits, stale edges, arity corruption,
  a substitution cycle, a consumed-tail successor, and synthetic size failure, all
  rejected before additional semantic mutation; and
- a 20,000-block adversarial-layout chain proving iterative discovery and one batch.

The standalone matrix below remains useful hardening context. Generated semantic
comparison across diamonds, asymmetric joins, loops, and calls now exists, as does a
handwritten real `None`-versus-`Baseline` lowered-target differential that exercises
fusion and CSE. Generated target cases and the pinned vanilla comparison remain Stage
5H work.

### Positive and non-match fixtures

- Empty and value-carrying two-block regions; long regions with results passed through
  multiple parameter lists; and parameter uses in surviving descendants.
- Swapped/multiple parameters proving simultaneous composition, plus later arguments
  that use earlier moved instruction results.
- Layout order deliberately different from CFG order, including a chain whose middle
  block appears before its head, proving maximal head discovery.
- A loop region whose final jump targets its surviving head and becomes a legal
  self-loop; a loop header with multiple incoming edges must not be consumed.
- Joins, entry destinations, self edges, two-arm same-target branches, different-arm
  arguments, attached unreachable incoming edges, and unreachable blocks.
- Multiple disjoint regions, final branches to other surviving heads, and stable
  survivor layout order.

### Ownership, atomicity, and provenance

- Exact before/after IDs and origins for head, moved instructions, final terminator,
  eliminated jumps, detached blocks, and detached parameters.
- Raw allocated-block scans proving unique instruction-container ownership and empty,
  unterminated fusion-consumed blocks.
- Byte/dump-identical rejection for malformed arity/type, stale edges, non-maximal or
  overlapping regions, duplicate raw instruction membership, a final edge to a
  globally consumed block, substitution conflict/cycle, and checked-size failure.

### Limits, determinism, and scale

- Budgets immediately before, inside, and after a region; an unfinished region is
  discarded and no suffix is fused, while earlier complete regions remain.
- A 20,000-block chain with adversarial layout order proving iterative traversal, one
  fact build, one replacement scan, one reserve per head, and one batch/layout retain.
- Stable output and representative choices independent of hash iteration.
- Generated small typed CFGs checked against a simple semantic/reference executor.
- Broader generated `None`-versus-baseline Minecraft lowering and pinned-server
  execution for representative parameterized chains and loops remains Stage 5H.

## Cross-compiler conclusions

- **rustc MIR:** its simplifier counts reachable predecessor edges, repeatedly merges a
  one-predecessor `Goto` successor, takes terminators to detect loops, and collects
  merged blocks before appending statements with one reserve. MDL adopts edge counts,
  moves, and batched concatenation, but uses an immutable snapshot because edits are
  disjoint and leaves stable allocated identities detached.
- **LLVM:** `MergeBlockIntoPredecessor` rejects missing/multiple predecessors and
  self-loops, folds single-entry PHIs, and explicitly updates dominator, loop,
  MemorySSA, and dependence structures. MDL has block parameters rather than PHIs and
  rebuilds analyses after its batch; it must still make invalidation explicit.
- **MLIR:** `Block::getSinglePredecessor` intentionally treats duplicate edges from one
  block as multiple, while `RewriterBase::mergeBlocks` replaces all source block
  arguments before moving operations and erasing the source. These are the closest
  models for MDL's exact-edge and simultaneous-parameter rules, except MDL detaches
  stable entities rather than deleting them.
- **GHC/Haskell Cmm:** `ContFlowOpt` concatenates an unconditional destination with one
  predecessor, processes from graph exits, and keeps predecessor counts updated while
  also performing shortcutting. MDL separates shortcutting/branch folding from fusion
  and replaces mutable incremental counts with maximal components from one snapshot.
- **Cranelift:** CLIF has typed block parameters and explicit branch arguments. Its
  current constant-phi removal separates summary construction, solution, and mutation.
  MDL follows that analysis/edit separation, but this source is not evidence for a
  Cranelift straight-line fusion algorithm.
- **Zig:** AIR encodes structured `block`, `loop`, `br`, and nested branch bodies rather
  than MDL's flat SSA CFG. It is a useful counterexample: its control-flow rewrite shape
  should not be transplanted into Core merely because AIR is compiler IR.
- **GCC:** GCC treats the CFG and instruction stream as coupled and recommends verified
  CFG manipulation hooks. MDL's consumer-specific atomic editor is the analogous
  boundary; direct mutation by the fusion pass would split responsibility incorrectly.

## Primary references

- rustc MIR CFG simplification source:
  <https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_mir_transform/simplify.rs.html>
- LLVM block merge implementation:
  <https://github.com/llvm/llvm-project/blob/main/llvm/lib/Transforms/Utils/BasicBlockUtils.cpp>
- LLVM's broader `SimplifyCFG` implementation:
  <https://github.com/llvm/llvm-project/blob/main/llvm/lib/Transforms/Utils/SimplifyCFG.cpp>
- MLIR `Block` predecessor semantics:
  <https://mlir.llvm.org/doxygen/classmlir_1_1Block.html>
- MLIR `RewriterBase` block inlining/merge implementation:
  <https://github.com/llvm/llvm-project/blob/main/mlir/lib/IR/PatternMatch.cpp>
- GHC Cmm control-flow optimizer:
  <https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/src/GHC.Cmm.ContFlowOpt.html>
- Cranelift IR block-parameter semantics:
  <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md>
- Cranelift constant-phi analysis/edit phases:
  <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/codegen/src/remove_constant_phis.rs>
- Zig AIR control-flow representation:
  <https://github.com/ziglang/zig/blob/master/src/Air.zig>
- GCC CFG maintenance guidance:
  <https://gcc.gnu.org/onlinedocs/gccint/Maintaining-the-CFG.html>
