# Dominance-Scoped Local CSE Pass

Status: **Implemented, pipeline-integrated, and fast differential-gated**

## Decision and boundary

Eliminate a reachable deterministic scalar instruction when an equivalent instruction
already dominates it. This is intentionally an EarlyCSE-shaped pass, not global value
numbering: Stage 5 does not add PRE, reassociation, expression synthesis, load/call CSE,
memory generations, or e-graphs here.

The current implementation gate is:

- `effects() == EffectClass::Pure`;
- `speculation() == Speculation::Always`;
- `result_equivalence() == ResultEquivalence::Structural`; and
- a complete `CoreExpressionKey` can be built.

Calls are opaque even if a future call analysis proves a particular callee pure. A new
operation is also opaque until the IR explicitly declares structural result equivalence
and the private key builder handles its entire payload and result contract.

`Always` is a deliberately conservative **Stage 5 editing restriction**, not a general
theorem about CSE. A dominating evaluation is not hoisted outside the duplicate's
control dependence, which is why LLVM EarlyCSE and MLIR CSE do not generally require
speculatability for ordinary dominated, effect-free expressions. Relaxing this gate
later requires a purpose-built atomic redundant-evaluation edit: the generic
`erase_discardable_inst_set` is shared with DCE and correctly requires `Pure + Always`.
Do not weaken DCE or silently conflate the two proof obligations.

## Closed structural key

`ir::core` owns the semantic classification
`ResultEquivalence::{Structural, Opaque}` and the closed
`OperandSymmetry::{Ordered, CommutativePair}` query. The latter is `CommutativePair`
for both additions and `I32Compare(Eq | Ne)`, and `Ordered` for every other current
operation. Canonicalization and CSE must consume this same IR fact; maintaining two
private lists of commutative operations is quiet semantic duplication. This follows
LLVM's `Instruction::isCommutative` and MLIR's `Commutative` operation trait without
importing either system's open trait machinery.

`opt::core::cse` owns a private, owned key:

```text
CoreExpressionKey {
    op: CseOpKey,                 // closed payload-bearing operation identity
    operands: SmallKeySeq<ValueId>,
    result_types: SmallKeySeq<CoreType>,
}
```

`SmallKeySeq<T>` is a pass-private `Zero | One(T) | Two(T, T) | Many(Box<[T]>)`
representation. It avoids a heap allocation for every current key while preserving a
correct fallback for a future wider operation. It is not a public IR container or a
new crate dependency.

`CseOpKey` is an exhaustive match over `CoreOp`; it is not a debug string, opcode
number, pointer, or serialization of `CoreOp`. Constants include their exact typed
payload, comparisons include the exact `I32Predicate`, and Boolean negation includes
its operation kind. Calls return no key. Result arity is represented by the complete
`result_types` sequence rather than a second potentially inconsistent count.

Resolve every operand through the already planned value substitution before keying.
Normalize operands inside the key builder, independently of whether canonicalization
ran:

- `I32AddWrapping` and `I32AddOverflowing` sort their resolved operand pair by
  `ValueId`; both complete result tuples are commutative under their Core contracts;
- `I32Compare(Eq | Ne)` sorts its resolved operand pair by `ValueId`;
- `I32Compare(SignedLt | SignedLe | SignedGt | SignedGe)` remains ordered and retains
  its exact predicate in Stage 5;
- every other current operation retains operand order.

Do not sort every binary operation, infer commutativity from equal operand types, or
depend on the canonicalizer having run. In particular,
`slt(a, b)` is not keyed as `slt(b, a)`. Predicate-swapping normalization such as
LLVM EarlyCSE performs is a possible later extension, but it must be one explicit
closed rule rather than folklore duplicated across passes.

Equality compares exactly the fields that hashing hashes. Source origins, allocated
`InstId`, result `ValueId`s, and block placement are excluded from expression
equality. The map value is the representative `InstId`; its ordered result IDs are
read from the immutable body on a hit, avoiding a heap allocation for every stored
result tuple. The verified complete result contract in the key makes the pairwise
replacement unambiguous.

## Frozen planning algorithm

Before constructing facts, checked-add the body's allocated block, instruction, and
value slot counts and compare that exact number with a typed
`CseAnalysisSlotLimit`. The current Core analyses allocate dense vectors over stable
allocated IDs, including detached historical data; attached/reachable counts are not
an honest proxy. If the configured optional-analysis limit is exceeded (including a
checked-add overflow), return the unchanged verified function with
`Ran(PassOutcome { completion: StoppedAtLimit(AnalysisSizeLimit), ... })`. `Skipped`
is reserved for a configured pipeline step that the runner does not invoke, such as
final cleanup with no enabling change. Do not allocate partial facts or publish a
partial CSE plan. The default size-relative limit is derived from the same stable pre-pass counts
and admits the whole valid body; an explicitly smaller limit exercises the fallback.
This is an optimization fallback, not a claim that process allocator OOM is
recoverable.

After that preflight, build fresh `PlacementIndex`, `ControlFlowGraph`, `Reachability`,
and `DominatorTree` from the verified pre-pass body. CSE runs after fusion, so reusing
fusion's CFG or dominance snapshot would be invalid. Process only entry-reachable
attached blocks; attached unreachable instructions are neither candidates nor
representatives. The final global value-replacement batch may still rewrite an
unreachable use of a reachable duplicate, which is harmless and matches the editor's
documented semantics. This makes the standalone pass safe even though the normal
pipeline should already have detached SCCP-proven unreachable blocks.

Derive dominator-tree children once in linear time. Iterate allocated `BlockId`s in
ascending entity order, skip the entry (whose immediate dominator is itself), and
append each reachable non-entry block to its immediate dominator's child list.
Ascending construction gives stable sibling order without a later `O(B log B)` sort.
A missing immediate dominator for a supposedly reachable non-entry block is an
internal contradiction, not a best-effort miss.

Walk that tree with an explicit stack; valid deep Core must not recurse on the Rust
call stack. Each frame stores the block, next child index, and scoped-table undo
checkpoint. Within a block, use authoritative instruction order. The table is one
hash map from `CoreExpressionKey` to representative `InstId` plus an undo log; entering
a block records a checkpoint and leaving it removes exactly the keys inserted below
that checkpoint. Do not clone a full map per block or search a vector of per-scope
maps from inner to outer.

For each reachable attached instruction:

1. Consume one typed scan unit before inspecting it. A limit stops before an
   instruction, never halfway through one instruction's result tuple.
2. Check the closed eligibility gate and build the key from resolved operands.
3. On a miss, insert `key -> instruction` in the current scope and record that key in
   the undo log. A typed `CseActiveEntryLimit` is checked before insertion. The root
   inspection and completed key construction are charged before this check; exhaustion
   stops before inserting or transforming that instruction and uses the same safe-prefix
   application rule as scan-limit exhaustion.
4. On a hit, read both immutable instructions, recheck equal complete result arity and
   types defensively, and record one `PlannedCse { duplicate, representative }`.
   Add every duplicate result to the planned substitution simultaneously and in
   contract order.

Only misses enter the table. Therefore a table representative is never itself marked
as a duplicate, every planned replacement points directly to a surviving instruction,
and representative chains cannot arise. Resolving planned substitutions is still
needed before key construction: it lets an expression using duplicate `B` match an
otherwise identical expression using `B`'s representative `A`. Resolve iteratively
and treat a cycle as an internal contradiction.

The scoped-table path proves cross-block dominance. Instruction order proves that a
same-block representative precedes its duplicate. No per-candidate call to
`DominatorTree::dominates`, no repeated `UseIndex`, and no mutation occurs during
discovery. Because the earliest visible entry is retained on every hit, the chosen
representative is the stable root-most/earliest instruction on the current dominator
path; hash-table iteration never chooses it.

## Multi-result and bounded-prefix atomicity

An instruction is the unit of CSE. `I32AddOverflowing` is either replaced as the
ordered pair `(sum, overflowed)` or not replaced at all. A hit never maps only the
currently used result, and a limit cannot split a result contract. This is stricter
than Cranelift's current e-graph GVN path, whose pure-node insertion asserts exactly
one result, and follows MLIR's replacement of the complete operation result range.

The default scan allowance is derived with checked arithmetic from the stable
pre-pass reachable-instruction count and permits the full traversal. A smaller
explicit compiler/test limit stops at a deterministic dominator-preorder prefix.
The default active-entry allowance is derived from that same instruction count and
permits every candidate to be simultaneously live; a smaller allowance bounds the
scoped table and undo log independently of scan fuel.
Already recorded instructions remain safe to replace even when they have later,
unvisited uses: the representative dominates the duplicate, and the duplicate's
definition dominates all of its valid reachable uses. Unwind table scopes after a
stop and apply only complete `PlannedCse` records. Limit exhaustion is
`StoppedAtLimit`, not optimizer failure.

Do not publish or apply a partially constructed mapping if key construction,
dominance-tree construction, checked size computation, or an invariant check fails.
Those are compilation failures. Ordinary ineligibility and hash misses are successful,
quiet non-transformations.

## Application, provenance, and invalidation

Convert planned instruction pairs to one final `ValueId -> Existing(ValueId)` batch
in deterministic body/instruction/result order. Preflight and apply:

1. one `replace_values_batch` containing every complete pairwise result mapping; then
2. one `erase_discardable_inst_set` containing every duplicate instruction in
   authoritative layout order.

After replacement, every result of every duplicate must have zero attached uses;
failure of that invariant is an internal error. Do not filter the erase set down to
"whichever results happen to be unused," because that would hide an incomplete
multi-result replacement. Each editor batch is atomic. If the second batch or final
verification unexpectedly fails, the owned compilation unit is discarded under the
optimizer failure contract; partially optimized Core is never returned.

The representative instruction and its origin survive. An eliminated origin is
non-semantic Core metadata and is retained only in an optional bounded applied remark
mapping eliminated site to representative site. `run_cse` uses no callback;
`run_cse_with_rewrite_sink` submits the duplicate/representative instruction
IDs, both complete ordered result slices, and both origins only after both editor
batches succeed. The runner-owned sink decides filtering and retention at submission
time. The enabled detail path streams successful pairs in stable allocated `InstId`
order from the already-retained dense representative table and erased raw instruction
records; it does not clone the application duplicate list or build a second per-site
correspondence vector. The ordinary disabled path does not invoke the callback at all.
Unlike GHC cost-centre ticks, Core origins do not affect execution and therefore are
not part of equality.

CSE changes no CFG edge or block membership, so its pre-pass dominator relation would
remain semantically valid in isolation. Its placement/use views and expression table
are nevertheless pass-local and are dropped after application. The following bounded
canonicalization/DCE cleanup rebuilds the facts it needs. Run that cleanup only when
fusion or CSE changed the function, and do not iterate the whole group to a fixed
point in Stage 5.

## Complexity and implementation shape

Let `AB`, `AI`, and `AV` be allocated stable block, instruction, and value slots, not
merely attached or reachable entities. Excluding dominance construction, the CSE
planner's own work is expected
`O(AB + AI + AV + attached operands)` with a normal hash table; every key and undo
record is inserted and removed at most once. The shared iterative
`DominatorTree::new` uses iterative simple Lengauer–Tarjan (SLT) with path-compressed
link-eval and dense intrusive buckets. For `AB` allocated block slots, `V`
entry-reachable blocks, and `E` attached CFG edge occurrences, dominance takes
`O(AB + (V + E) log V)` time, `O(AB + V)` temporary space beyond the already-built
CFG, and `O(AB)` retained space. CSE builds it once for planning and the generic
value-replacement validator builds it again when the replacement batch is nonempty;
the separate counters expose both sequential builds, while their peak scratch does
not add together. Final replacement/application adds
`O(AI + AV + attached uses)`. Peak temporary space is
`O(AB + AI + AV + V + active expressions + planned duplicates)`, subject to the typed
CSE limits. A constant-hash adversarial test proves correct equality under collisions
but does not turn worst-case hash-table time into a linear guarantee.

Use Rust's normal map lookup APIs; map iteration is forbidden for semantic output,
report order, representative choice, or batch order. Randomized hash seeds may alter
bucket layout but not output. Do not add a deterministic weak hasher merely to make
internal bucket order reproducible.

The Stage 5 implementation remains a small concrete module. Do not introduce a public
value-numbering framework, generic operation trait objects, cross-pass analysis cache,
or e-graph substrate for seven current Core operations.

## Test matrix

### Eligibility and key contract

- An exhaustive fixture covers every current `CoreOp`; all constants/arithmetic/
  comparison/Boolean operations have the reviewed key and calls are opaque.
- Constant payload, comparison predicate, operand order, result type, and result
  arity differences miss.
- Both adds match with swapped operands even when canonicalization is omitted.
- `eq`/`ne` match with swapped operands through the shared symmetry query; the four
  signed ordering predicates do not. Predicate-swapped pairs such as `slt(a,b)` and
  `sgt(b,a)` remain distinct in Stage 5.
- Equality implies equal hash. A test-only constant hasher forces all keys into one
  collision path, and randomized map seeds produce byte-identical Core and remarks.

### Dominance and scoping

- Same-block earlier/later, dominating-block, diamond siblings, join, loop header/body,
  self-loop, deep dominator chain, and wide sibling tree.
- A key inserted in one sibling is popped and cannot represent the same key in another
  sibling.
- Entry-rooted traversal never chooses an attached unreachable instruction; a global
  batch may update its use of a replaced reachable value without making it executable.
- Block children are stable by `BlockId`; instruction order chooses the representative
  within one block.
- A transitive-operand fixture has expression `C` consume duplicate `B` of `A` and
  match the form keyed with `A` without mutating during discovery.

### Results, limits, and provenance

- `I32AddOverflowing` replaces both results together when zero, one, or both results
  are used; payload/type/arity corruption is rejected rather than partially applied.
- Limits at zero, before a miss, before a hit, immediately after a multi-result hit,
  and before a descendant yield the exact deterministic valid prefix.
- Allocated-slot preflight counts detached blocks/instructions/values and produces an
  unchanged `StoppedAtLimit(AnalysisSizeLimit)` pass outcome without constructing dense facts;
  active-entry exhaustion stops before insertion and applies only the valid prefix.
- Every planned representative survives; replacement chains and cycles are rejected
  by assertions/error paths.
- The representative origin survives, eliminated origins appear only when applied
  remarks are requested, remark truncation does not change optimization, and normal
  no-change execution allocates no detail records.

### Scale and semantics

- 20,000 repeated expressions and a 20,000-block dominator chain complete through the
  real CSE path without host recursion. CSE's scoped walker separately covers a
  synthetic 20,000-child dominator tree, while the shared dominance implementation
  covers a verifier-valid 60,001-block mirrored-fan CFG whose 20,000 leaf immediate
  dominators are the entry.
- Counters pin one planning fact snapshot and one planning dominator build, one visit
  per scanned instruction, one replacement-validation dominator build when the value
  batch is nonempty, and one explicit post-replacement use audit, replacement batch,
  and erasure batch on changed runs. This remains exact for a future structural
  zero-result operation and exposes the generic editor's second dominance validation
  rather than hiding it behind one ambiguous fact-build counter.
- A small reachable body with large detached allocated history proves that limits and
  complexity use allocated ID slots rather than attached counts.
- A second standalone CSE invocation is unchanged.
- Generated small verified typed CFG families compare `None` and `Baseline` Core
  results and ordered call effects across varied topologies. Standalone CSE fixtures
  cover exact limit prefixes and verification; one handwritten full-pipeline fixture
  compares real lowered targets while exercising CSE and fusion. A generated
  cross-layer limit-prefix target matrix is deferred until those private test
  boundaries have a deliberate shared harness.

## Cross-compiler findings

- **LLVM EarlyCSE** uses an iterative dominator-tree DFS and scoped hash tables, and
  normalizes only operations whose semantics explicitly permit it. It deliberately
  leaves harder redundancy to GVN. This supports the scoped, nonrecursive Stage 5
  shape, the IR-owned symmetry query, and the refusal to grow PRE here.
- **LLVM GVN/NewGVN** are broader value-numbering systems that include partial
  redundancy and memory reasoning. Their existence is evidence to keep this pass's
  name and promise narrow, not to add a half-GVN framework.
- **MLIR CSE** hashes/equates structural operations while ignoring locations, replaces
  the complete result range, and walks CFG dominance with explicit scope frames. It
  admits memory-effect-free operations without an independent speculation gate,
  supporting the distinction between Stage 5's conservative editor gate and the
  general CSE proof.
- **rustc MIR GVN** interns symbolic values, distinguishes nondeterministic constants,
  and tests that a reusable local's assignment dominates the replacement site. Core's
  much smaller closed SSA vocabulary can use scoped structural keys, but must retain
  the same explicit determinism/opacity boundary.
- **GHC Core CSE** applies its substitution before reverse-expression lookup and takes
  care not to erase semantically relevant ticks or inlining behavior. The former
  motivates resolved operands; the latter confirms that excluding Core origins is
  valid only because their contract is non-semantic.
- **Cranelift's e-graph pass** combines scoped GVN with availability and extraction,
  but its pure-node insertion currently assumes one result. Stage 5 should borrow the
  scoped availability discipline, not its e-graph machinery or single-result
  assumption.
- **Zig AIR** uses a closed instruction-tag representation and tag-specific liveness,
  including sparse side storage for exceptional shapes. It does not provide a generic
  CSE framework to copy; the useful lesson here is to keep semantic/key decisions
  exhaustive and concrete for the current IR.

## Primary references

- LLVM EarlyCSE implementation:
  <https://github.com/llvm/llvm-project/blob/main/llvm/lib/Transforms/Scalar/EarlyCSE.cpp>
- LLVM GVN overview:
  <https://llvm.org/docs/Passes.html#gvn-global-value-numbering>
- LLVM NewGVN implementation:
  <https://github.com/llvm/llvm-project/blob/main/llvm/lib/Transforms/Scalar/NewGVN.cpp>
- MLIR CSE utility implementation:
  <https://github.com/llvm/llvm-project/blob/main/mlir/lib/Transforms/Utils/CSE.cpp>
- MLIR CSE pass and preserved-dominance contract:
  <https://github.com/llvm/llvm-project/blob/main/mlir/lib/Transforms/CSE.cpp>
- MLIR's `Commutative` operation trait:
  <https://mlir.llvm.org/docs/Traits/#commutative>
- LLVM's intrinsic instruction commutativity query:
  <https://llvm.org/docs/doxygen/classllvm_1_1Instruction.html>
- rustc MIR GVN implementation and operational contract:
  <https://github.com/rust-lang/rust/blob/master/compiler/rustc_mir_transform/src/gvn.rs>
- GHC Core CSE implementation:
  <https://github.com/ghc/ghc/blob/master/compiler/GHC/Core/Opt/CSE.hs>
- Cranelift scoped GVN/e-graph implementation:
  <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/codegen/src/egraph/mod.rs>
- Cranelift GVN multi-result regression fixtures:
  <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/filetests/filetests/egraph/multivalue.clif>
- Zig AIR representation:
  <https://github.com/ziglang/zig/blob/master/src/Air.zig>
- Zig AIR liveness implementation:
  <https://github.com/ziglang/zig/blob/master/src/Air/Liveness.zig>
- Lengauer and Tarjan, *A Fast Algorithm for Finding Dominators in a Flowgraph*:
  <https://doi.org/10.1145/357062.357071>
- Georgiadis, Tarjan, and Werneck, *Finding Dominators in Practice* (simple
  Lengauer–Tarjan and path-compressed link-eval):
  <https://jgaa.info/index.php/jgaa/article/view/paper119>
