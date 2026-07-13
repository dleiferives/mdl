# Cheap Core Canonicalization Pass

Status: **Implemented, pipeline-integrated, and fast-gated**

## Purpose and boundary

Put the current small Core vocabulary into one locally simpler form before SCCP and
CSE. This is a best-effort optimization, never a legality or lowering prerequisite.
It is a closed exhaustive match over `CoreOp`, not a generic pattern registry.

The pass performs only exact, target-independent rewrites that do not duplicate
computation, reassociate arithmetic, fold calls, or inspect Minecraft costs. SCCP
owns general evaluation of operations whose operands are constants. DCE owns generic
unused-instruction removal.

This separation follows LLVM's distinction between target-independent
canonicalization and cost-driven combining, MLIR's requirement that pipelines remain
correct without canonicalization, rustc's small exhaustive MIR peephole sweep, and
Zig AIR's closed tagged-operation analyses. GCC's much larger `match.pd` machinery is
useful evidence for sharing thousands of patterns, but is deliberately too large for
the current seven-operation Core.

## Closed rewrite table

Rules are tried in this order. Each root gets at most one value replacement or one
operand rewrite.

1. `bool.not`:
   - `bool.not(true|false)` becomes the opposite typed constant;
   - `bool.not(bool.not(x))` becomes `x`.
2. Addition identities:
   - wrapping `i32` addition with zero in either operand becomes the other operand;
   - overflowing `i32` addition with zero in either operand replaces the complete
     result tuple with `(other_operand, false)`.
3. Reflexive signed comparison:
   - `eq(x,x)`, `sle(x,x)`, and `sge(x,x)` become `true`;
   - `ne(x,x)`, `slt(x,x)`, and `sgt(x,x)` become `false`.
4. Remaining operations whose Core-owned
   `OperandSymmetry == CommutativePair` put operands in a deterministic order:
   nonconstants before constants, then resolved `ValueId` or signed/Boolean constant
   payload, with the original `ValueId` as the final tie-breaker. This covers both
   additions and `I32Compare(Eq | Ne)`; signed ordering predicates remain ordered.
   Equal keys retain their input order.
5. A branch becomes a jump only when both arms have the same destination and their
   complete effective argument tuples are equal. Same destination with different
   effective arguments is not equivalent.

For double negation, inspect the immutable raw operand's defining instruction before
resolving its planned replacement. This makes triple and quadruple negation collapse
correctly even when the inner negation is also scheduled for removal. All other
tests use effective operands obtained through the virtual replacement map.

"Reflexive" comparison means the two operands resolve to the same value identity; it
does not quietly turn this pass into general two-constant evaluation. Effective
branch-argument equality may compare identical typed virtual constants because that
is the exact value that the later replacement batch will install.

Both add operations are commutative under their declared Core contracts, including
the overflow flag, and equality/inequality comparisons are symmetric. Consume the
same exhaustive `OperandSymmetry` query as CSE rather than duplicating a private
allowlist. No rule relies on Rust debug/release overflow behavior: use
explicit `i32` wrapping semantics and the predicate table above.

## Frozen planning algorithm

Before allocating facts, compare allocated block, instruction, **and value**
cardinalities with a typed canonicalization table limit. Placement/CFG facts are dense
over blocks/instructions and the virtual replacement table is dense over stable
`ValueId`s; omitting detached value history would make the cap dishonest. If the limit
is too small, return unchanged `StoppedAtLimit`. Then build those facts from the verified
pre-pass body. Do not mutate while matching. Visit only entry-reachable blocks in the
CFG's deterministic reverse-postorder and instructions in block execution order.
For valid reachable SSA, this visits every operand definition before its users.
Attached unreachable code is left for SCCP reachability cleanup; a global value batch
may still rewrite its uses of a reachable definition safely.

Maintain a pass-private map:

```text
VirtualValue = Existing(ValueId) | Constant(TypedCoreConstant)
replacement: ValueId -> VirtualValue
```

Resolve chains iteratively with path compression; reject a cycle as an internal
contradiction. Never recurse through IR depth. A replacement with `Existing` must be
a transitive operand of the rewritten root. A replacement with `Constant` is
materialized at that root's definition site. This **backward-substitution invariant**
is the reason a single forward planning sweep is sufficient: no current rule can
create a new dependency on an unvisited definition.

Record commutative operand plans using the original operand identities, ordering them
by their resolved virtual keys. The later value-replacement batch resolves those
identities after operand order is installed. After all reachable instructions have
been visited, scan their terminators in the same block order and compare branch arms
through the completed virtual map. A replacement jump keeps one chosen arm's raw
argument IDs and the branch terminator's origin; the value batch resolves the raw
arguments later.

The complete virtual map is a planning fact, not automatically the editor payload.
Project the planned terminator/operand rewrites and scan uses once before value
application. Retain a source mapping only when its value has a terminator use or an
instruction use outside the planned erase set. A mapping used solely by instructions
that will be erased need not materialize at all. This is essential with append-only
stable IDs: manufacturing a temporary constant for every internal node in a long
folded chain and then DCE-detaching it would permanently inflate every later dense
analysis. Path compression and multi-result proofs still use the complete virtual map;
only the final live-out editor subset is filtered.

This intentionally does not copy LLVM InstCombine or MLIR's greedy driver. Those
drivers mutate, create operations, and need to revisit dynamic users. This closed
pass plans only backward substitutions and idempotent operand ordering against a
frozen body. If a future rule creates an operation, replaces a value with a
non-operand definition, or requires a newly formed parent pattern, that rule must go
to a later pass or trigger an explicit redesign to a real dynamic-use worklist.

## Application and ownership

Preflight and apply the proven plans in this order:

1. `set_terminators_batch` for identical-arm branches;
2. `rewrite_inst_operands_batch` for surviving symmetric operations;
3. one current-use scan to filter the complete virtual map to mappings with a use
   outside the planned erase set, followed by `replace_values_batch` for that final
   existing-value and typed-constant subset; and
4. `erase_discardable_inst_set` for instructions whose **complete** result contract
   was replaced and now has no use outside the erase set.

The ordering is semantic: terminators and operands retain raw identities until the
single value batch resolves them. The erasure preflight observes the post-replacement
uses. An overflowing add always plans both result replacements together, even when
only one result was originally used. Generic erasure still requires `Pure + Always`.

Each live-out generated constant is inserted deterministically at the eliminated
instruction's definition site and inherits that instruction's origin. Planning-only
constants with no post-projection use outside the erase set are never allocated. The
surviving existing value keeps its own origin. Do not globally hoist or unique
constants in Stage 5. Optional
detail remarks may record eliminated origin to representative origin, but aggregate
statistics do not allocate one record per rewrite.

Each editor batch is independently atomic. A later unexpected batch failure consumes
the owned compilation unit as specified by the optimizer API; no partially optimized
program is returned.

## Termination, limits, and complexity

The current pass is a bounded sweep, not a fixed-point engine:

- every reachable instruction and terminator root is visited once;
- value-replacing rules remove the root or replace it by a terminal constant;
- commutative ordering uses one total order and is idempotent; and
- branch-to-jump cannot reverse within this pass.

Consequently, there is no separate successful-rewrite fuel and no user requeueing.
The dense-table limit and root-scan allowance are distinct units. Compute the default
scan allowance with checked arithmetic from
`reachable_instruction_count + reachable_block_count`; it permits the complete
sweep. A smaller explicit compiler limit may stop at a deterministic prefix and
apply only already proven plans, returning `StoppedAtLimit`. Entity-capacity failure
for planned constants is preflighted by the value batch and remains an ordinary
compilation failure. Statistics saturation never changes control flow.

Expected analysis time is `O(allocated blocks + allocated instructions + allocated
values + attached CFG edges + visited operands)`, and analysis scratch is
`O(allocated blocks + allocated instructions + allocated values)` plus virtual plans.
Stable detached identity makes an attached-only memory claim false. Editor application
adds its documented scans. The pass must not hide a repeated whole-body editor call per
match. A second standalone invocation on its output must report no change.

## Tests

- One exact fixture and one non-match for every rule and both zero orientations.
- All six comparison predicates and `i32::{MIN,MAX}` around the wrapping and
  overflowing contracts.
- Overflowing-add fixtures with only the sum used, only the flag used, both used, and
  neither used; the replacement remains a complete two-result contract.
- Triple and quadruple negation, add-zero chains, and a comparison whose operands
  become identical only through virtual replacement.
- A long folded constant/identity chain whose only live-out value allocates only the
  required live-out constant, with no temporary constant arena history for internal
  nodes.
- Commutative ordering for two nonconstants, constant/nonconstant in both orders,
  distinct equal-valued constant definitions, `eq`/`ne` in both orientations,
  ordered signed predicates, and stable equal-key input.
- Same-target branches with raw-equal, virtual-equal, and unequal argument tuples;
  different-target branches never fold.
- A reachable CFG whose `block_order` is not definition-before-use, proving RPO—not
  layout order—is the planning authority. Include attached unreachable blocks and
  prove they are neither visited nor made a correctness prerequisite.
- Exact entity-capacity preflight and no mutation on rejected constant allocation.
- A small dense-table cap with large detached block/instruction/value history, proving
  unchanged fallback before fact allocation.
- A deliberately small scan cap proving deterministic, verified prefix improvement.
- Pass idempotence, repeated-run determinism, and byte-stable canonical dumps. This
  pass uses dense vectors rather than a hash table, so a fake hash-seed test would not
  exercise anything.
- A 20,000-instruction identity chain with counters proving one fact construction,
  one root visit, and one application scan per batch rather than quadratic rescans.
- Standalone and complete-pipeline before/after semantic checks through the bounded
  test Core evaluator, plus Core-to-Minecraft lowering as a legality/integration check.
  Lowering success alone is not evidence of semantic equivalence.

## Primary references

- LLVM InstCombine contributor guide, especially canonicalization versus
  target-dependent profitability and `InstructionSimplify`'s no-new-instruction
  boundary: <https://llvm.org/docs/InstCombineContributorGuide.html>
- LLVM's dynamic instruction worklist, including explicit user requeueing after
  immediate mutation (the heavier mechanism this frozen planner does not need):
  <https://github.com/llvm/llvm-project/blob/main/llvm/include/llvm/Transforms/Utils/InstructionWorklist.h>
- MLIR canonicalization design: best effort, bounded, convergent, and never required
  for pipeline correctness:
  <https://mlir.llvm.org/docs/Canonicalization/>
- MLIR developer rule against recursion on unbounded IR depth:
  <https://mlir.llvm.org/getting_started/DeveloperGuide/>
- rustc MIR `InstSimplify`, an optional closed peephole sweep with explicit rule order:
  <https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_mir_transform/instsimplify.rs.html>
- rustc MIR CFG simplification, including exact duplicate switch-target cleanup and
  explicit analysis invalidation:
  <https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_mir_transform/simplify.rs.html>
- Zig AIR's closed tagged instruction vocabulary and separate legalization,
  liveness, and verification modules:
  <https://codeberg.org/ziglang/zig/src/branch/master/src/Air.zig>
- Zig AIR liveness, showing explicit per-tag operand/death handling rather than a
  generic rewrite registry:
  <https://codeberg.org/ziglang/zig/src/branch/master/src/Air/Liveness.zig>
- Zig AIR verifier and legalizer, which keep validation and target feature expansion
  separate from the semantic instruction vocabulary:
  <https://codeberg.org/ziglang/zig/src/branch/master/src/Air/Verify.zig>,
  <https://codeberg.org/ziglang/zig/src/branch/master/src/Air/Legalize.zig>
- GHC simplifier monad's size-derived tick accounting and retained rewrite history:
  <https://github.com/ghc/ghc/blob/master/compiler/GHC/Core/Opt/Simplify/Monad.hs>
- GHC user guide on bounded simplifier iterations and size-relative tick factors:
  <https://ghc.gitlab.haskell.org/ghc/doc/users_guide/using-optimisation.html>
- GCC match-and-simplify, including capture equality and the convention that
  constants are second in commutative forms:
  <https://gcc.gnu.org/onlinedocs/gccint/Match-and-Simplify.html>
- LLVM's closed instruction commutativity query:
  <https://llvm.org/docs/doxygen/classllvm_1_1Instruction.html>
- MLIR's `Commutative` operation trait:
  <https://mlir.llvm.org/docs/Traits/#commutative>
