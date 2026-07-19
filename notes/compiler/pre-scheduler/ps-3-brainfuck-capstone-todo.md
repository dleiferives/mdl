# PS-3 Brainfuck Capstone Checklist

Status: **ready to begin PS-3.0 from [`ps-2-handoff.md`](ps-2-handoff.md)**

Authoritative design: [`ps-3-brainfuck-capstone-plan.md`](ps-3-brainfuck-capstone-plan.md).

## PS-3.0 — Accept the handoff

- [ ] Confirm every PS-2 exit criterion and public API required by the frozen
      Brainfuck contract.
- [ ] Freeze package modules, case schema, default limits, and output adapter.
- [ ] Confirm whether the actual-player-held-book test is required or explicitly
      deferred after the holder-independent proof.
- [ ] Confirm the capstone contains no raw command, private storage/objective, sealed
      intrinsic forgery, or compiler-only test hook.
- [ ] Record discovered missing capabilities as PS-2 work instead of hiding them in
      the application.

Gate: PS-3 is composition work over accepted public facilities.

## PS-3A — Tape and opcode model

- [ ] Implement opcode and byte abstractions with the frozen semantics.
- [ ] Implement the two-sided zero-extending tape through standard-library zipper
      operations.
- [ ] Test wrapping, left/right movement, revisit, and copy independence in the Core
      evaluator.
- [ ] Compile tape cases through all policies and compare server observations where
      physical list behavior is exercised.

Gate: tape behavior is independent from parsing and books.

## PS-3B — Parser

- [ ] Implement text traversal and ignored-character filtering.
- [ ] Implement normalized opcode construction and source-position tracking.
- [ ] Implement bracket validation and the selected jump representation.
- [ ] Test empty/comments, ASCII, Unicode ignored text, nested brackets, unmatched
      brackets, and maximum limits.
- [ ] Compare constant/runtime and evaluator/vanilla results.

Gate: arbitrary accepted semantic program text becomes a validated program or typed
parse error.

## PS-3C — Fuel-bounded interpreter

- [ ] Implement all eight instructions using public arithmetic, list, aggregate, and
      control-flow facilities.
- [ ] Consume fuel exactly according to the frozen dispatch rule.
- [ ] Implement input consumption/end behavior and byte output accumulation.
- [ ] Return completed, step-limit, and invalid-program states distinctly.
- [ ] Test zero/exact/one-short/surplus fuel, simple/nested loops, I/O, and tape
      movement in the evaluator.
- [ ] Compile and run the interpreter tier under all four policies.

Gate: pre-normalized and runtime-string programs execute correctly without a book.

## PS-3D — Book and Minecraft adapters

- [ ] Implement typed book acquisition from the supported exact-one holder/slot.
- [ ] Implement page conversion/joining through public platform/standard APIs.
- [ ] Keep missing/wrong/oversized book results separate from parser failures.
- [ ] Implement selected output effect without replacing semantic byte observations.
- [ ] Run clientless holder/book-value cases on pinned vanilla.
- [ ] Verify executor attribution and execution frame where the output operation
      depends on them.

Gate: a controlled written-book value drives the ordinary parser/interpreter path.

## PS-3E — Connected-player boundary

- [ ] Research and select an existing narrow test-client/bot dependency or external
      fixture compatible with the pinned Java 26.2 server.
- [ ] Pin its version and document offline-mode credentials/protocol assumptions.
- [ ] Connect one player, wait for server-visible readiness, place a written book in
      the selected slot through supported server commands, and invoke the pack.
- [ ] Observe the same semantic output and attribution as the holder-independent
      case.
- [ ] Terminate the client reliably and preserve client/server logs on failure.
- [ ] Keep this suite opt-in and out of the default workspace tests.

Gate: required only if PS-3.0 freezes actual-player-held input as an exit criterion.

## PS-3F — Full corpus and policy differential

- [ ] Encode every case with tier, program/book, input, fuel, expected result/output,
      and server/client requirement.
- [ ] Include empty, wrap, movement, I/O, simple/nested loops, comments/pages,
      bracket errors, fuel boundaries, and size limits.
- [ ] Add one recognizable showcase program within the synchronous contract.
- [ ] Compile all four policy products and execute compatible cases in batched server
      lifecycles.
- [ ] Compare normalized observations and produce concise case/policy state diffs.
- [ ] Assert normal completion/error cleanup and explicit recovery after a separate
      abnormal command-limit case.

Gate: the complete frozen semantic corpus passes through its declared authorities.

## PS-3G — Cost, inspection, and handoff

- [ ] Record exact pack files/lines/bytes and selected recipe census.
- [ ] Record conservative command-sequence, redirect/fork, function, score, NBT, and
      macro work for the bounded capstone configurations.
- [ ] Prove strict accepted/rejected configurations at selected pinned-server hard
      boundaries without semantic instrumentation.
- [ ] Run wall-time/reload/cache experiments only through the measurement protocol.
- [ ] Confirm no unused standard-library modules or private runtime helpers are
      emitted.
- [ ] Audit source-to-HIR/Core/lowering/target/pack explanations for representative
      operations.
- [ ] Update the roadmap, testing notes, semantic ambiguities, and Stage 9 handoff.
- [ ] Mark Stage 8.5 complete only after the PS-1/2/3 audits remain green together.

Gate: Brainfuck is a reproducible compiler capability proof and Stage 9 receives a
concrete persistent-work client rather than an abstract scheduler wish list.
