# PS-3 Brainfuck Capstone Checklist

Status: **complete on 2026-07-19; see [`ps-3-handoff.md`](ps-3-handoff.md)**

Authoritative design: [`ps-3-brainfuck-capstone-plan.md`](ps-3-brainfuck-capstone-plan.md).

## PS-3.0 — Accept the handoff

- [x] Confirm the accepted PS-2 public API and frozen Brainfuck contract.
- [x] Freeze seven package modules, JSON case schemas, default limits, and output marker adapter.
- [x] Retain the PS-2.0 connected-player deferral; require the holder-independent exact-one proof.
- [x] Audit out raw commands, private names, intrinsic forgery, and compiler-only hooks.
- [x] Adapt to the public cross-module type surface without adding a compiler exception.

Gate: passed; PS-3 is ordinary composition work.

## PS-3A — Tape and opcode model

- [x] Implement opcode and wrapping byte abstractions.
- [x] Implement the two-sided zero-extending two-list tape.
- [x] Evaluate wrap, movement, revisit, tail orientation, and copy behavior independently.
- [x] Compile the complete application through all four policies.

Gate: passed independently from books.

## PS-3B — Parser

- [x] Implement UTF-16 tail traversal and ignored-character filtering.
- [x] Construct normalized opcodes with normalized positions.
- [x] Validate nested brackets before execution and retain unmatched positions.
- [x] Cover empty/comments, Unicode, nested/unmatched brackets, and exact/exceeded limits.
- [x] Compare the application with an independent reference under all policies.

Gate: passed; runtime text becomes a validated reverse opcode list or typed status.

## PS-3C — Fuel-bounded interpreter

- [x] Implement all eight instructions with public operations.
- [x] Consume exactly one fuel unit per dispatched opcode.
- [x] Implement input, EOF-zero, ordered byte output, and output bounds.
- [x] Return completed, fuel, parse, and resource-limit states distinctly.
- [x] Compare the complete returned state for all 30 semantic cases.
- [x] Compile/evaluate under all four policy products.

Gate: passed without a book or Brainfuck intrinsic.

## PS-3D — Book and Minecraft adapters

- [x] Read an exact-one typed holder/main-hand written book.
- [x] Inspect all static page indices 0..99 and preserve page order through reverse normalization.
- [x] Keep empty/wrong/unsupported conversion as `NO_PROGRAM`, distinct from parse and fuel errors.
- [x] Retain semantic byte output and add a typed reader-attributed `say` marker.
- [x] Run split-page, wrong-item, and malformed-book cases on pinned Java 26.2.
- [x] Prove attribution to `MDL_PS3_READER` under all four policies.

Gate: passed in one clientless server lifecycle.

## PS-3E — Connected-player boundary

Not applicable to the PS-3 exit. PS-2.0 explicitly selected the controlled
inventory-capable non-player holder and deferred a pinned client dependency,
credentials/protocol handling, and player lifecycle automation. This remains
optional future breadth and was not disguised as capstone work.

## PS-3F — Full corpus and policy differential

- [x] Encode target-independent and book-adapter cases in machine-readable JSON.
- [x] Cover empty, wrap, movement, I/O, loops, Unicode/comments/pages, brackets, fuel, and bounds.
- [x] Produce the recognizable `A` showcase with 108 dispatches.
- [x] Compile all four policies and run the compatible book path for all four in one server.
- [x] Compare normalized complete state and concise case names against an independent oracle.
- [x] Retain Stage 8's separate command-limit abort/recovery test as the abnormal authority.

Gate: passed; semantic fuel exhaustion is not Minecraft interruption.

## PS-3G — Cost, inspection, and handoff

- [x] Pin exact pack files/functions/lines/bytes/maximum-line/trace metrics for every policy.
- [x] Pin command census plus physical homes, realizations, materializations, and recipe work.
- [x] Record `NoFiniteBoundProven(PositiveCycle)` for sequence work and `ProvenWithin` forks.
- [x] Classify strict deployment rejection/static fuel proofs as accepted deferred breadth.
- [x] Make no wall-time performance claim; use only the semantic server protocol.
- [x] Prove zero raw commands, zero function macros, and zero recursive activation edges.
- [x] Retain source/Core/lowering/target/pack products through the production compilation facade.
- [x] Update the roadmap, testing notes, ambiguities, and Stage 9 state handoff.
- [x] Keep PS-1, PS-2, PS-3, workspace, and pinned-server gates green together.

Gate: passed. Stage 8.5 is complete and Stage 9 has a concrete persistent-work client.
