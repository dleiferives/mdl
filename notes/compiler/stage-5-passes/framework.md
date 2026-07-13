# Core Optimizer Framework

Status: **Implemented, integrated, and fast-gated through Stage 5D**

## Purpose and boundary

Provide the owned, verified, bounded substrate that runs the five Stage 5 Core passes.
This layer contains no Minecraft knowledge and no pass-specific rewrite rules. The
closed pipeline and all five concrete consumers are implemented and proven together.

Public API:

```text
optimize_core(
  CoreProgram,
  &SourceContext,
  &CoreOptimizationOptions,
) -> Result<CoreOptimizationOutput, CoreOptimizationFailure>

CoreOptimizationOutput { verified program, deterministic report }
CoreOptimizationLevel = None | Baseline
```

The input is consumed. Failure drops every partially changed body and returns only
typed context plus deterministic byte-capped malformed-safe text. `None` performs one
whole-program verification and returns the same owned program; a second scan would
prove nothing new because it performs no mutation. There is no public pass registry or
experimental level. `Baseline` is exposed only because its complete reviewed 5C/5D
pipeline is wired; there is no empty/no-op placeholder level.

## Closed pipeline

`CorePassKind` identifies the five implementations.
`CorePipelineStep` identifies
stable invocations such as initial versus final canonicalization. An exhaustive
dispatcher groups the complete local pipeline by stable `FunctionId`; local passes may
read immutable declarations but cannot mutate signatures or initiate whole-program
analysis.

Invocation-local `PassOutcome` contains `changed`, typed completion, and one inline
statistics variant matching the pass kind. It is immediately merged into a fixed
`CorePipelineStepSummary`; normal reports retain no function-by-step matrix. Counters
use `StatisticCount::Exact(u64) | Saturated`. Detailed function/site records go only
through the filtered, capped runner-owned remark sink.

## Editor batches

Keep `FunctionEditor::new` as the checked public constructor and add a private safe
trusted-open path usable only after optimizer verification. Stage 5 passes use only:

```text
replace_values_batch(ValueId -> Existing(ValueId) | Constant(TypedCoreConstant))
rewrite_inst_operands_batch(InstId -> complete operands)
erase_discardable_inst_set(InstId set)
set_terminators_batch(BlockId -> Terminator)
detach_unreachable_blocks()
```

Each operation performs complete preflight before mutation, builds required facts
once, and applies in authoritative order. Mixed replacement chains ending in a
constant materialize a separate constant at each replaced value's own definition
site; they never share a later definition that fails dominance. Entity-ID capacity is
preflighted for both new instructions and values. A representable entity-capacity
failure aborts the owned compilation unit; system allocator OOM follows Rust's process
policy rather than pretending to be a recoverable compiler diagnostic.

Every nontrivial replacement source and final existing-value endpoint must have an
attached definition. Duplicate sources and self/cyclic mappings are rejected. Constant
instructions inherit the replaced parameter or defining instruction origin and are
planned in stable definition-site/`ValueId` order: after conceptual block parameters
and before the first instruction, or immediately before the replaced instruction
result's definition. Detached historical uses are never rewritten.

Terminator and operand batches preserve the raw `ValueId`s supplied by their caller;
when canonicalization composes batches, the subsequent value batch resolves those
identities before the erasure batch checks post-replacement uses. Erasure never trusts
the choosing pass: it rechecks unique attached ownership and the exhaustive
discardability query, then performs one linear current-body closure scan over
instruction and terminator uses. It retains raw instruction data, stable IDs, origins,
and detached containers.

Terminator batches are assessed simultaneously. Their preflight builds the projected
CFG/reachability/dominance relation with every override installed, then rechecks all
attached instruction and projected-terminator uses before mutation. Checking only the
new terminators against the old CFG is unsound because a new edge can make old code
reachable or introduce a dominance-bypassing path. Non-entry self-loops remain valid
Core; fusion-specific self/overlap/ambiguous-edge policy belongs to 5D.

The consumer-specific `PreparedJumpFusion<'editor>` is added with the 5D fusion pass,
not prebuilt into this generic 5B substrate. It may use the private trusted editor
boundary, but must retain its exclusive borrow through fact construction and
application so another mutation cannot stale its snapshot.

The editor is not a transaction system. Each batch is atomic; if a later successful
batch is followed by an invariant failure, the consumed compilation unit is dropped.

## Verification, diagnostics, and limits

Mutating `Baseline` verifies the whole program on entry and exit. `None` verifies once
and returns its unchanged owned value. Tests/CI and debug builds verify the current
function after each executed step; optimized builds may select boundary verification,
with a developer verify-each override. No batch secretly runs a whole-function
verifier, so the policy changes real work.

Limits have typed units per pass. Optional-limit fallback is defined by the pass plan;
verification, entity capacity, and invariant construction never degrade to best
effort. Passes that build dense entity-indexed facts first preflight allocated stable
ID slots, including detached history; a too-small optional table limit returns
unchanged before allocation. The runner supplies `Derived` or an explicit typed
override; each concrete pass derives its exact default once from the metadata it
already scans (for example reachable roots or SCCP edges/uses). The framework does not
duplicate full-body scans or force unlike passes into a coarse shared formula. Default execution allocates no remarks, timing records,
dumps, or per-function statistics containers. Complete dumps are explicit developer
output and are not replayable Core serialization. A failure snapshot retains only a
byte-capped UTF-8 prefix, but exact omitted-byte metadata still requires one full
streaming traversal. The guarantee covers retained text bytes, not total failure
memory or constant construction time: malformed-safe traversal uses linear dense
attachment-bit scratch, and verifier diagnostics remain proportional to the findings
rather than the snapshot cap.

## Implementation order

1. Add owned options/output/failure types and `None` behavior.
2. Add the trusted-open editor path and effective verification policy.
3. Implement and corrupt-test value/constant/operand batches.
4. Implement and corrupt-test erasure/terminator/unreachable batches.
5. Extend Core verification with raw instruction-container ownership across all
   allocated blocks; detached membership does not count as executable placement, but
   no `InstId` may be listed by two block vectors.
6. Implement the real 5C/5D pass consumers.
7. Only then replace exported generic pass injection with closed kind/step dispatch,
   fixed summaries, inline statistics, a remark sink, timing hooks, and skip states. A
   no-op dispatcher is not an acceptable framework test double.
8. Run 20,000-entity scan/allocation counters and public API review.

## Tests

- Consumed failure never exposes a body; successful output always verifies.
- `None` preserves exact Core and performs the documented verifier calls.
- Every batch rejection is byte/dump identical before and after.
- Mixed existing/constant chains, definition-site placement, dominance, types,
  multi-results, entity limits, and malformed input.
- Duplicate instruction IDs across attached/detached block containers are rejected;
  ordinary unreachable detachment retains unique historical ownership.
- Verify-each versus boundary verifier-call counters.
- No-change tiny functions allocate no report-detail containers.
- Stable step summaries distinguish repeated cleanup and saturate counters explicitly.
- Default execution performs no dump rendering; capped failure snapshots are stable.

## Primary references

- rustc MIR pass manager and validation modes:
  <https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_mir_transform/pass_manager.rs.html>
- GHC's explicit Core pipeline algebra:
  <https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/GHC-Core-Opt-Pipeline-Types.html>
- MLIR pass scope, function grouping, failure, verification, and instrumentation:
  <https://mlir.llvm.org/docs/PassManagement/>
- LLVM analysis scope and invalidation trade-offs:
  <https://llvm.org/docs/NewPassManager.html>
- GCC's explicit pass properties and bookkeeping limitations:
  <https://gcc.gnu.org/onlinedocs/gccint/Pass-manager.html>
