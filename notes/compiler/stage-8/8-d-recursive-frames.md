# Stage 8D — Recursive SCC Activation Frames

Status: **complete for the scalar direct-score ABI**

## Purpose

Stage 8D replaces the recursive-call rejection with a verified synchronous frame
protocol for existing scalar functions. Dynamic frames are confined to reachable
recursive strongly connected components (SCCs); acyclic calls and serial fork
children keep static score storage.

This tranche proves the first real score→NBT→score multiple-realization path.

## Current repository boundary

Before Stage 8D the legality audit already:

- constructs the reachable internal call graph;
- computes SCCs iteratively without host recursion;
- classifies self or multi-function recursive components;
- chooses deterministic provenance from the earliest internal recursive edge; and
- emitted `lower.recursive-call-abi` before target construction.

Core verification, ambient-context analysis, demand, target-cost solving, and
function-reference inventories already accepted recursive graphs conservatively.
The implemented activation analysis reuses the iterative graph/SCC product, marks
only internal recursive-component edges, and freezes exact caller-live spill homes
on each call occurrence. The old rejection was removed only after physical
preflight and compiler-generated vanilla execution passed.

Reuse that SCC product or factor it into an owned shared analysis. Do not add a
second recursive-graph algorithm with subtly different reachable-edge rules.

## Activation classification

Classify reachable functions/edges:

```text
FunctionActivation =
  StaticAcyclic
  | RecursiveMember { scc: RecursiveSccId }

CallActivation =
  StaticDirect
  | EnterRecursiveScc { scc }
  | RecursiveInternal { scc }
  | LeaveToStaticCallee
```

Rules:

- an acyclic caller→acyclic callee remains the current fixed ABI;
- an acyclic caller entering a recursive SCC pushes one frame for the callee;
- every edge whose caller and callee are in the same recursive SCC pushes a child
  frame;
- a recursive member calling a static callee may use that callee's ordinary fixed
  homes because no graph path from the callee can re-enter the recursive member (or
  they would share an SCC); and
- an exported function has separate public root and internal activation entries when
  root recovery differs from internal calling.

Do not force all transitive callees of a recursive function onto the stack.

## Frame schema

Use one compiler-private command-storage list per active root. The implemented
scalar compatibility ABI keeps parameters and results in fixed score homes, so a
runtime frame contains only caller values that are actually live across that exact
recursive call:

```snbt
{
  frames: [
    {s41: 91, s42: 0b}
  ]
}
```

Each `sN` key is derived from the verified `PhysicalStorageId` of one
`RecursiveSpill` declaration. Empty frames begin as `{}` and acquire only the fields
selected for that call occurrence. No SCC/function/argument/result discriminants are
emitted because the current direct-score protocol never reads them.

The final recursive-spill handle carries both that physical storage identity and its
verified Core scalar type. Emission selects NBT `byte` versus `int` from this frozen
physical decision; it does not re-derive the width independently from the legacy
flattened home table.

Every field name/path is compiler-generated from typed physical-storage identities,
not source text. The retained declarations record:

- calling function and recursive call-instruction owner;
- exact spill fields owned by each recursive call occurrence;
- Boolean byte normalization;
- initialization requirements; and
- exact type, owner call occurrence, and deterministic ordinal.

The structured target verifier and renderer validate final static paths and command
lengths once generated resource names are available.

Frames use NBT `int` for `Int32` and `byte` for canonical `Bool`. Score→NBT int with
scale 1 preserves signed i32; byte stores are safe only after the source score is
proven `0|1`. NBT→score through `data get` is exact for those stored domains.

## Structured target primitives

All emitted actions must use structured target IR:

```mcfunction
data modify storage <runtime> frames append value <typed-empty-frame>
execute store result storage <runtime> frames[-1].s41 int 1 run scoreboard players get <i32-score>
execute store result storage <runtime> frames[-1].s42 byte 1 run scoreboard players get <bool-score>
execute store result score <score> run data get storage <runtime> frames[-1].s41
data remove storage <runtime> frames[-1]
```

Add or extend structured Minecraft IR nodes, builders, verifier, renderer, dump,
syntax census, command contracts, cost analysis, and preflight. Never lower a frame
action through raw text.

No macro is required because every field and `[-1]`/`[-2]` path is static.

## Call protocol

### Caller planning

For each stack-requiring call occurrence compute:

- semantic arguments and demanded results;
- current score realizations used by each argument;
- score realizations live after the call that may be clobbered by re-entering the
  SCC;
- child frame schema/fields;
- outgoing score/frame materializations;
- callee parameter-load actions;
- result/spill incoming transfer; and
- frame pop/continuation placement.

The clobber set comes from the recursive component's physical home inventory and the
callee's reachable physical writes, intersected with caller liveness. Do not spill a
caller score merely because it is live if the recursive subtree cannot write that
storage.

### Ordered runtime protocol

1. Append a typed child frame.
2. Store every caller realization that is both live after the call and clobbered by
   the recursive subtree into child `spills`.
3. Copy indexed semantic arguments into the callee's fixed score ABI.
4. Invoke the callee's ordinary internal entry.
5. The semantic body executes under the unchanged Minecraft context and writes its
   fixed result scores.
6. Return to the caller without popping the child.
7. Restore exact caller-live spills, then define demanded call-result realizations
   from the callee's pinned result scores.
8. Remove `frames[-1]`.
9. Continue the caller.

Implementation evidence showed that a separate frame-argument/result wrapper is not
required for the scalar fixed-score ABI: every reentrant edge preserves precisely
the caller score realizations it can clobber before writing callee parameters. A
source `return` exits only the callee's generated function; the caller continuation
still owns restore/result/pop. This is smaller than a wrapper protocol while keeping
the same well-bracketed ownership. Frame argument/result modes remain reserved for a
future ABI that cannot use pinned scalar score slots.

The caller owns push and pop. This gives it access to results and makes ownership
well-bracketed.

### Ordered incoming transfer for the direct-score ABI

The selected ABI never places parameters or results in a frame field. A recursive
call therefore has two disjoint source domains after return: immutable child-frame
spill fields and the callee's pinned result scores. Frame sources remain readable
until pop, so they cannot participate in a score-copy cycle. The verifier rejects:

- one destination defined by two incoming values;
- a result destination aliasing a different caller value still live after the call;
- a result destination overlapping a spill destination;
- a direct self-call result home overlapping a spill home;
- reading a child field after pop; and
- popping before every demanded result/spill read.

The ordered protocol restores spills before copying pinned results. The separation
proof makes that order equivalent to the intended simultaneous boundary transfer.
General typed frame-source parallel copies become necessary only if a future ABI
places parameters/results in frames; that work is explicitly deferred rather than
implemented as unused machinery.

## Wrapper and worker structure

Use distinct resource roles:

```text
PublicRootEntry(function)       // optional reset/recovery, external ABI
InternalActivation(function)    // load args, ordinary call body, store results
SemanticEntryBlock(function)    // current generated body/control graph
```

The implemented scalar path does not allocate `InternalActivation`: recursive calls
use the ordinary internal entry and keep all cleanup in the caller continuation.
This remains valid only while results use pinned scores and target `return` unwinds
the current callee rather than its caller. A later frame-field ABI must introduce the
wrapper/result-store epilogue described above.

Internal source calls to an exported function always use `InternalActivation` (or
its static entry), never `PublicRootEntry`.

Functions outside recursive SCCs should not gain wrappers unless required by an
entry/ABI boundary. Unused wrapper roles allocate no resources.

## Root entry, poison, and recovery

### Successful execution

At successful root completion the frame list is empty. A debug assertion function
may test this in server fixtures; production should not add an unconditional check
unless selected by a runtime-safety profile.

### Command-limit abortion

If the target stops execution mid-call:

- some world effects may already have occurred;
- score homes may contain partial values;
- a child frame may remain;
- the caller pop/restore may not execute; and
- no source result is valid.

This is a deployment-contract violation, not a recoverable source return. The
compiler must report it honestly.

### Implemented Stage 8 external contract

Stage 8 selected explicit recovery instead of resetting every public entry:

- normal exports use the current external scoreboard parameter/result ABI;
- successful recursive roots leave the frame list empty;
- command-sequence abortion may leave private residue and partial score state;
- `<namespace>:__mdl/load` is the explicit recovery entry and replaces the frame
  list with `[]` before ordinary initialization;
- external callers must recover/reload before another export after known abnormal
  termination; and
- internal calls never invoke load/recovery.

Clearing compiler-private frames repairs only scratch ownership. It cannot undo world
writes from an aborted root.

## Recursion depth and target limits

The plan publishes:

```text
ActivationDepthContract::CallerBounded
```

Cost analysis should expose:

- fixed push/load/store/call/result/restore/pop cost per recursive edge;
- exact static spill count per call occurrence;
- known or unknown recursive invocation count;
- command-sequence/fork compatibility status; and
- compiler-private NBT growth per active depth.

Do not use `max_command_sequence_length` as a claimed semantic depth: each recursion
level consumes several operations and the exact remaining budget depends on the path.

No runtime depth guard is added until the language has selected failure/trap
semantics. A compile configuration may be used for analysis/reporting, but exceeding
it cannot silently change program behavior.

## Optimization boundaries

Stage 8D permits only correctness-preserving local improvements:

- omit dead result fields/actions;
- omit a spill when the subtree cannot clobber its storage;
- reuse current parallel-copy scratch; and
- omit all frame infrastructure from nonrecursive programs.

Tail recursion elimination, recursion-to-loop conversion, frame scalarization across
the whole SCC, macro paths, and profile-guided conventions are later work.

## Physical preflight and verification

`PhysicalPreflight` inventories every selected scalar physical recipe:

- frame append schema;
- score→NBT and NBT→score bridge;
- internal function call;
- pop/reset command.

It validates type-specific constant and score↔frame counts plus exact local
sequence/fork costs before resource allocation. Structured target construction,
verification, rendering, and command-line validation own the later checks that need
allocated resource/path spelling.

Independent verification recomputes:

- SCC membership and recursive edge classification;
- frame ownership/schema completeness;
- exact clobber∩liveness spill set;
- argument/result arity/types;
- internal-entry/control reachability;
- push/pop balance on every normal path;
- no public-root reset on internal edges;
- materialization equivalence and Boolean normalization;
- type-specific recipe counts, local cost, effect/context contracts; and
- constructed target commands and exact correlations.

## Scheduling boundary

Core has no schedule or yield operation in Stage 8, so a typed program cannot
suspend with a live synchronous frame. Unsafe raw commands retain unknown effects
and cannot make their continuation part of MDL's typed frame protocol. Stage 9 must
introduce persistent continuation state rather than reusing these synchronous tail
frames.

## Diagnostics

Replace the broad recursion error with precise failures:

- unsupported recursive type/operation physical requirement;
- recursive frame analysis limit;
- missing legal score/frame bridge;
- frame schema/path/command too large;
- call transfer alias/liveness conflict;
- wrapper cannot guarantee cleanup for selected control recipe;
- nested external root contract violation; or
- recursion depth remains caller-bounded (report/contract, not normally an error).

Every diagnostic labels the recursive edge and relevant value/use/ABI position.

## Tests

### Fast structural

- Direct self recursion with zero/one/multiple parameters and results.
- Mutual recursion with asymmetric signatures.
- Recursive branch base case and early return.
- Dead result and value not live across the call produce no field/spill.
- Live value in a non-clobbered foreign function home produces no spill.
- Call-result/coalesced-home conflict is rejected or assigned differently.
- Recursive SCC calling static leaf keeps the leaf static.
- Acyclic caller entering SCC and recursive export root.
- Internal call to exported recursive function avoids public reset.
- Corrupted SCC/frame/schema/bridge/transfer/wrapper/pop/resource/correlation tables.
- 20,000-function chains plus large recursive SCCs without host recursion.

### Target structural

- Exact structured data/store/get/remove commands and no raw nodes.
- No frame storage/resource in nonrecursive artifacts.
- Complete trace correlation for every push/bridge/call/restore/pop.
- Deterministic schema/path/resource names under all policies.

### Official server

- Direct and mutual recursion with known terminating inputs.
- Multiple recursion depths and multiple returned values.
- Values live across recursion prove restoration.
- Recursion inside each child of a many-context run scope.
- Ordinary return/fallthrough cleanup.
- Deliberate command-limit abortion followed by the selected recovery/root behavior.
- Empty selector/fork rejection does not leave frames.
- All four optimization-policy combinations agree.

## Gate

Stage 8D completes when recursive programs execute correctly through stack frames,
only recursive SCCs pay frame costs, every normal path is balanced, abnormal
termination is honestly contracted, and current nonrecursive output remains free of
runtime stack support.

## References

- [MLIR bufferization function-boundary recursion limitation](https://mlir.llvm.org/docs/Bufferization/)
- [MLIR ownership-based function ABI](https://mlir.llvm.org/docs/OwnershipBasedBufferDeallocation/)
- [GHC Cmm register/stack argument assignment](https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/src/GHC.Cmm.CallConv.html)
- [GHC worker/wrapper](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/using-optimisation.html)
- [`lists/callbacks-and-frames.md`](../../mcfunction/lists/callbacks-and-frames.md)
- [`numeric-conversion.md`](../../mcfunction/numeric-conversion.md)
- [`crates/mdl-compiler/src/lower/minecraft/call.rs`](../../../crates/mdl-compiler/src/lower/minecraft/call.rs)
- [`crates/mdl-compiler/src/lower/minecraft/audit.rs`](../../../crates/mdl-compiler/src/lower/minecraft/audit.rs)
- [`crates/mdl-compiler/src/lower/minecraft/transfer.rs`](../../../crates/mdl-compiler/src/lower/minecraft/transfer.rs)
