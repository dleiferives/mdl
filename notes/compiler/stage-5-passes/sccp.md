# Sparse Conditional Constant Propagation Pass

Status: **Implemented, pipeline-integrated, and fast-gated**

## Purpose and phase-1 boundary

Prove typed scalar constants and executable Core control-flow edges together, then
apply only a completed solution. The pass is intraprocedural and analysis-first:

- function entry parameters are runtime-dependent;
- every result of an internal call is runtime-dependent, even when its operands are
  constant;
- calls and their effects remain in place;
- function signatures and block-parameter contracts do not change; and
- the solver never mutates Core or invokes another optimization.

This is classic optimistic SCCP over the current, verified Core vocabulary. It is
not a generic dataflow framework, abstract interpreter, range analysis, predicate
analysis, interprocedural constant propagation, or rewrite engine. Wegman and
Zadeck's key warning applies directly: optimistic facts are not safe transformation
facts if propagation stops early.

## Findings from other compilers

The implementation should borrow invariants, not surface architecture:

| System | Relevant design | Stage 5 conclusion |
| --- | --- | --- |
| Wegman–Zadeck SCCP | SSA value facts and executable-flow facts advance monotonically; an interrupted optimistic solution may be wrong. | Keep the all-or-nothing completion boundary and discard every solver fact at a limit. |
| LLVM SCCP | Separates executable blocks/edges from value states, revisits users only after widening, solves before rewriting, and pessimistically resolves facts left unknown in executable code. Its feasible-edge key is `(source,destination)`, which fits LLVM PHIs. | Use the same monotone/event-driven shape, but not LLVM's edge key: Core permits two branch arms with the same destination and different argument tuples. |
| GCC SSA-CCP | PHIs join only arguments on executable **edge objects** and substitution occurs after propagation. | Give each Core successor arm its own identity and join the corresponding argument tuple. |
| MLIR dataflow | Dead-code and sparse constant analyses are separate subscriptions in a shared solver; lattice changes enqueue dependent program points. Simulated constant folding is not allowed to mutate the analyzed operation. | Explicitly connect executable-edge and value events, but keep one small SCCP-specific solver and immutable Core input. |
| rustc MIR | Uses a flat constant lattice, explicit complexity limits, a fixpoint analysis, and a later patching traversal. Its current pass is dense place dataflow rather than SCCP. | Reuse the flat-lattice and analyze-then-apply lessons, not the dense per-program-point state. |
| Zig | AIR is per-function, typed, tag-exhaustive, and stored in compact parallel tables; most compile-time evaluation happens earlier in Sema rather than in an AIR SCCP pass. | Use typed entity-indexed scratch and an exhaustive `CoreOp` transfer match; Zig is not evidence for adding a second generic optimizer IR. |
| GHC (Haskell) | Cmm dataflow makes bottom, join, transfer, and rewrite boundaries explicit. | Keep the same conceptual separation in ordinary Rust values without importing a generic lattice/rewrite abstraction for one pass. |

Cranelift's block-argument SSA is also representationally close to Core, but it does
not change the central choice here: a Core edge is a successor occurrence, not merely
a pair of blocks.

## Typed value lattice

Use one flat lattice cell per allocated `ValueId`:

```text
                    Overdefined
                   /     |      \
             Bool(false) ... I32(n)
                   \      |      /
                       Unknown
```

`Unknown` is bottom: the solver has not yet obtained a constraint from executable
flow. `Overdefined` is top: the runtime value is not one provable constant. Distinct
constants are incomparable. The only legal state changes are therefore:

```text
Unknown -> Constant
Unknown -> Overdefined
Constant(c) -> Overdefined
```

Joining the same constant is unchanged; joining distinct constants or anything with
`Overdefined` produces `Overdefined`. Call this operation `join`, consistently with
the lattice order above. A block parameter joins facts from executable incoming
edges; it does not use a separate, oppositely named "meet" operation.

The Rust representation must make a constant's Core type explicit, for example:

```text
Unknown | BoolConstant(bool) | I32Constant(i32) | Overdefined
```

Every join validates the cell's immutable `CoreType`; a Boolean fact can never enter
an `i32` cell or vice versa. Tests cover commutativity, associativity, idempotence,
monotonicity, and all type-rejection paths.

## Original-graph snapshot and edge identity

Build solver metadata from the verified, attached body before propagation. Do not
use `ControlFlowGraph` predecessor pairs as SCCP edge identities: that analysis stores
only predecessor `BlockId`s and cannot distinguish this valid Core terminator:

```text
branch %condition, ^join(%true_arg), ^join(%false_arg)
```

Define a private semantic edge identity instead:

```text
SccpEdgeId {
    source: BlockId,
    successor_index: u8,
}
```

Successor zero is a jump target or the `then` arm; successor one is the `else` arm.
The edge catalog stores its destination and original argument tuple. Catalog edges in
attached block order and successor order. `UseSite::EdgeArgument` already carries the
same successor and argument indexes, so it can map to the exact catalog entry.

Scratch consists of:

- a lattice cell per allocated value;
- executable and queued bits per allocated block;
- queued bits per allocated instruction and attached terminator;
- executable and queued bits per semantic edge;
- queued bits per attached edge argument;
- the existing attached-use index; and
- one stable FIFO event queue.

Detached historical entities receive no executable facts and are never visited.
Dense entity-indexed outer tables are acceptable because Core retains stable IDs,
but all allocation sizes are checked against the table cap before constructing the
use index, edge catalog, queued bits, or worklist. Count attached successor arms,
edge arguments, and uses with a read-only preflight scan and checked arithmetic.

## Exact transfer functions

An instruction is evaluated only after its block becomes executable. Evaluate all
of its result facts from one immutable operand-fact snapshot, then join every result
into its existing cell before the queue services newly scheduled dependents. Never
overwrite a cell with a supposedly more precise fact.

Use an exhaustive private match over `CoreOp`:

| Operation | Transfer |
| --- | --- |
| `BoolConstant(c)` | `BoolConstant(c)` |
| `I32Constant(c)` | `I32Constant(c)` |
| `BoolNot(x)` | Negate a Boolean constant; propagate `Unknown` or `Overdefined` otherwise. |
| `I32AddWrapping(x,y)` | If both are constants, use `i32::wrapping_add`; if either is `Overdefined`, the result is `Overdefined`; otherwise it is `Unknown`. |
| `I32AddOverflowing(x,y)` | If both are constants, use `i32::overflowing_add` and produce the sum and flag independently. If either operand is the constant zero, the sum receives the other operand's lattice fact and the overflow result is `false`. Otherwise an `Overdefined` operand makes both results `Overdefined`; unresolved operands leave both `Unknown`. |
| `I32Compare(p,x,y)` | Fold two constants using the signed predicate. Identical SSA operands fold to `true` for `eq`, `sle`, and `sge`, and `false` for `ne`, `slt`, and `sgt`, regardless of their lattice fact. Otherwise an `Overdefined` operand makes the result `Overdefined`; unresolved operands leave it `Unknown`. |
| `Call(_)` | Mark every result `Overdefined` as soon as the call's block is executable. Do not inspect the callee body or wait for argument facts. |

The zero rule for overflowing addition is useful in standalone SCCP tests even though
the preceding canonicalizer normally removes it. It also proves that multi-result
facts are independent: the overflow flag may be constant while the sum is not.

The exhaustive match is a maintenance gate. Adding a `CoreOp` must force an explicit
SCCP decision. Do not call the Minecraft emitter to evaluate Core, use host `+` that
can panic in a debug build, infer semantics from operation names, or use an in-place
canonicalizer as an analysis oracle. Shared pure scalar helpers may be extracted only
when they preserve the exact Core contracts and are tested at `i32::MIN/MAX`.

## Event-driven solver

Use an explicit event enum and `VecDeque`; no propagation path may recurse on the
host stack. A minimal event set is:

```text
VisitBlock(BlockId)
VisitInstruction(InstId)
VisitTerminator(BlockId)
ActivateEdge(SccpEdgeId)
PropagateEdgeArgument(SccpEdgeId, argument_index)
```

Per-kind queued bits suppress duplicates without changing authoritative event order.
Processing is:

1. Initialize every lattice cell to `Unknown` and every edge/block to non-executable.
2. Set all entry-block parameters to `Overdefined`, then mark the entry executable
   and enqueue `VisitBlock(entry)`.
3. `VisitBlock` enqueues its attached instructions in instruction order followed by
   its terminator. It runs only on the block's first executable transition.
4. `VisitInstruction` snapshots operands, applies the exact transfer, and joins each
   result. It is ignored defensively if its block is not executable.
5. A changed value examines its attached dependent uses in existing `UseIndex` order.
   An instruction operand or branch condition queues its consumer only when the
   consumer block is executable; an argument of an already executable edge queues
   the matching `PropagateEdgeArgument`. Returns have no intraprocedural fact
   consumer. `VisitBlock` will seed consumers in a block that becomes executable
   later, so notifying currently dead blocks is unnecessary.
6. `VisitTerminator` activates the jump edge; activates exactly the selected branch
   edge for a Boolean constant; activates both branch edges for `Overdefined`; and
   activates no branch edge for `Unknown`. Return and unreachable terminators have no
   successor event.
7. `ActivateEdge` marks that exact arm executable once, queues each of its argument
   propagations in argument order, and marks/queues the destination block if this is
   its first executable incoming edge.
8. `PropagateEdgeArgument` reads the source value's current fact and joins it into
   the corresponding destination block parameter. If the source later widens, its
   edge-argument use schedules the same propagation again.

Step 8 is essential. Joining an edge argument only when its edge first becomes
executable is unsound when the argument is still `Unknown` and later becomes a
constant or `Overdefined`.

All queries use the original terminators, operands, definitions, placement, and use
index. Solver facts never observe a partially rewritten CFG.

## Pessimistic completion

Queue quiescence alone is not a publication proof. Maintain a bit for whether each
executable block has undergone pessimistic finalization. After the ordinary queue
drains, process every executable-but-unfinalized block in attached layout/result
order. Promote **every** remaining `Unknown` block parameter or instruction result
in those blocks to `Overdefined`, mark the blocks finalized, and resume the event
queue. Newly executable blocks become unfinalized. Repeat until both the event queue
and unfinalized-block set are empty. Never rescan a finalized block: its facts cannot
return to `Unknown`, so the fallback remains linear even if resolving one unknown
branch exposes a long chain of blocks.

This is deliberately broader than resolving only unknown branch conditions. An
unknown returned value or executable edge argument is also not a completed runtime
fact. LLVM similarly resolves unknown instruction facts in executable blocks before
consuming its solution; Core block parameters require the analogous treatment.

Current verified Core has no `undef`, every supported scalar transfer is exhaustive,
calls become overdefined, and entry parameters are seeded. Consequently normal
fixtures should need zero pessimistic resolutions. Keep an aggregate counter and
assert that expectation across the current valid corpus, while retaining the
conservative release behavior as protection against a solver omission or future
operation. Unit-test the completion routine directly with synthetic scratch states;
do not add a fake Core operation merely to make an end-to-end unresolved fixture.

A private `CompletedSccpSolution` is constructible only when:

- the event queue is empty;
- no definition in an executable block remains `Unknown`;
- every executable jump edge is executable;
- a constant branch has its selected edge executable; and
- an overdefined branch has both successor arms executable.

These checks are solver invariants, not optional diagnostics.

## Limits and failure behavior

Use two typed limits with separate counters:

```text
SccpTableLimit   // values, blocks, instructions, uses, edges, edge arguments
SccpEventLimit   // dequeued events plus pessimistic lattice resolutions
```

`table_entries` is the checked, reported aggregate
`values + blocks + instructions + uses + edges + edge_arguments`. The same cap must
also admit every individual bounded allocation. In particular, the stable FIFO has
the exact slot bound `instructions + 2 * blocks + edges + edge_arguments`, because a
block visit and its terminator visit have separate queued bits. Therefore preflight's
private admission requirement is
`max(table_entries, fifo_slot_bound)`, not their sum. Keep the published statistic as
`table_entries`; the maximum exists only to decide whether allocation may begin. A
one-block, result-less `return` body is the important boundary: it reports one table
entry but needs two FIFO slots, so an explicit cap of two passes and a cap of one
stops before solver allocation.

Default event fuel is derived with checked arithmetic from the stable pre-pass counts
and the flat lattice height. Each value changes at most twice, each edge becomes
executable once, and each changed value can notify only its attached uses. Retain
separate counters for lattice transitions, dequeued events, first edge activations,
edge-argument propagations, and maximum queue length so scale tests can check the
claimed bound.

If either configured limit is reached or its derived bound cannot be represented,
drop the queue, tables, executable bits, and every prospective decision. Return
unchanged `StoppedAtLimit`; do not publish constants, branch folds, or dead blocks.
The report may retain only bounded aggregate counters and the stable limit reason.

Representable entity-capacity failure while applying a completed solution is a
compilation failure, not a missed optimization. System allocator exhaustion follows
Rust's process policy and is not reported as recoverable SCCP exhaustion.

## Immutable decisions and one application

After completion, derive a compact immutable decision record from the original graph
and drop the lattice/use/worklist scratch. The record contains only:

- proven constant values in executable blocks that have at least one use in
  solution-executable code surviving the planned branch folds, and are not already
  defined by the identical constant operation;
- executable branches with a constant condition and their selected original target
  plus argument tuple;
- attached blocks proven non-executable; and
- aggregate counters and bounded optional remarks.

Apply once through the Stage 5B batches:

1. finish every pass-owned decision payload and checked size before opening the
   editor; each editor batch then performs its own complete atomic preflight against
   the current body immediately before committing;
2. batch-replace constant branches with jumps, preserving the selected target's raw
   argument IDs and the original terminator origin;
3. batch-replace proven constant uses at deterministic definition sites, including
   those raw IDs in the newly installed jumps;
4. detach blocks unreachable in the rewritten CFG; and
5. let the following `Pure + Always` DCE remove newly unused scalar instructions.

Do not erase calls, delete block parameters, mutate while solving, rerun SCCP against
partially edited Core, or retain solver tables in `CoreOptimizationReport`. Two arms
to the same block remain distinct decisions, so selecting one keeps exactly that
arm's arguments. Do not reverse steps 2 and 3: installing an original argument tuple
after value replacement would reintroduce uses that the value batch never saw.

The optimizer owns the whole input program and drops it after an application failure;
individual Stage 5B batches still preflight atomically. There is no pass-level clone
or rollback copy. Stage 5B deliberately does not expose reusable “preflight now,
commit later” tokens whose body facts could become stale across the preceding batch.

## Complexity and determinism

Let `Va`, `Ia`, and `Ba` be allocated values, instructions, and blocks, and let `I`,
`B`, `E`, `A`, and `U` be their attached executable-layout subset, semantic successor
arms, edge arguments, and attached uses. Expected time is
`O(Va + Ia + Ba + I + B + E + A + U)` for dense-table initialization plus the current
flat-lattice events, followed by one decision derivation and the documented batch
scans. Scratch is `O(Va + Ia + Ba + E + A + U)`; stable detached identities make an
attached-only claim false. It is released before returning the optimization report.

Entity/layout order, successor order, result order, argument order, and `UseIndex`
order determine all events and decisions. Hash-map iteration may not choose work,
representatives, remarks, or dumps. Deep CFGs and long use chains use queues/stacks,
never Rust recursion.

## Tests

### Lattice and transfer tests

- Exhaustive lattice join tables per type plus algebraic-law property tests.
- Every current `CoreOp`, every signed predicate, identical operands, and
  `i32::{MIN,MAX}` with wrapping/overflow boundaries.
- Overflowing-add cases in which only the overflow result is constant, and cases in
  which both results later widen.
- Calls with zero, one, and multiple results; all results become `Overdefined` while
  the call remains attached.

### Flow and convergence tests

- Constant and overdefined diamonds, loops with entry seeds, unreachable
  predecessors, nested branches, return/unreachable terminators, and backedges.
- Two arms from one branch to the same destination with equal and unequal argument
  tuples; assert separate edge facts and the correct block-parameter joins.
- An executable edge whose argument transitions `Unknown -> Constant -> Overdefined`;
  assert the destination parameter follows both transitions.
- Multiple parameters on one edge and one parameter with many executable incoming
  edges, including conflicting constants.
- Synthetic completion-state tests for unknown parameters, instruction results,
  edge arguments, returns, and branch conditions; current valid Core fixtures assert
  zero pessimistic resolutions.

### Completion, limit, and application tests

- Stop at zero fuel, one event, exactly one event before completion, and exact
  completion; every incomplete case leaves the canonical Core dump unchanged.
- A late executable edge that invalidates an earlier constant, proving an incomplete
  optimistic table is never applied.
- Table-cap rejection with a small attached body and large detached identity history;
  assert rejection happens before solver scratch allocation.
- Existing constant definitions and proven-but-unused values do not cause redundant
  constant materialization.
- Branch folding preserves the selected argument tuple and terminator origin; dead
  effects disappear only because their blocks are unreachable, while live calls stay.
- Entity-capacity application failure returns no optimized program.

### Independent and scale tests

- A test-only dense round-robin SCCP oracle over small generated verified Core CFGs;
  compare every value fact, semantic edge fact, and executable block, not just the
  final dump. Keep its scheduling structurally independent from the sparse solver.
- Differential `None` versus baseline Core evaluation/lowering on deterministic
  terminating fixtures, plus the existing official-server boundary corpus where
  vanilla behavior is relevant.
- Deep and wide 20,000-entity functions, high fan-in block parameters, long use
  chains, and same-target branch arms. Assert transition/event counters and maximum
  queue size stay within the documented checked bounds and no host recursion occurs.
- Repeated-run determinism. The solver uses dense tables and a FIFO rather than a hash
  map, so deliberately changing a nonexistent hash seed would be a fake gate.

## Primary references

- Wegman and Zadeck, *Constant Propagation with Conditional Branches*:
  <https://research.ibm.com/publications/constant-propagation-with-conditional-branches--1>
- LLVM SCCP solver implementation:
  <https://llvm.org/doxygen/SCCPSolver_8cpp_source.html>
- LLVM scalar SCCP transformation, which consumes solver results after solving:
  <https://llvm.org/doxygen/Scalar_2SCCP_8cpp_source.html>
- GCC SSA-CCP implementation and executable-edge PHI handling:
  <https://gcc.gnu.org/git/?p=gcc.git;a=blob;f=gcc/tree-ssa-ccp.cc;hb=HEAD>
- MLIR dead-code/executable-edge analysis:
  <https://mlir.llvm.org/doxygen/DeadCodeAnalysis_8cpp_source.html>
- MLIR sparse constant propagation:
  <https://mlir.llvm.org/doxygen/ConstantPropagationAnalysis_8cpp_source.html>
- MLIR monotone lattice/dataflow tutorial:
  <https://mlir.llvm.org/docs/Tutorials/DataFlowAnalysis/>
- rustc MIR dataflow constant propagation:
  <https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_mir_transform/dataflow_const_prop.rs.html>
- Zig AIR representation:
  <https://codeberg.org/ziglang/zig/src/branch/master/src/Air.zig>
- Zig AIR liveness tables:
  <https://codeberg.org/ziglang/zig/src/branch/master/src/Air/Liveness.zig>
- GHC Cmm dataflow lattice, transfer, fixpoint, and rewrite API:
  <https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/src/GHC.Cmm.Dataflow.html>
- Cranelift IR block-argument and control-flow reference:
  <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md>
