# PS-2 Aggregate Values

Status: **implemented for bounded nominal structs through explicit 1:N scalar lowering**

## Goal

Support the fixed interpreter/parser state required by the capstone while preserving
Stage 8's separation of semantic values, storage, realizations, ABI, and activation
lifetime.

The broader starting research remains in
[`../../aggregate-values-plan.md`](../../aggregate-values-plan.md). This dossier
assigns its first required client to Stage 8.5.

## Initial source semantics

Recommended first slice:

- nominal fixed-layout structs;
- fields containing `Bool`, `Int32`, or already-supported aggregate/list handles as
  those types become available;
- deterministic declaration/layout order for diagnostics and serialization, without
  making physical layout source-visible;
- explicit construction with every required field;
- typed field projection;
- whole-value assignment and call/return copy semantics;
- source `var` updates lowered to new SSA aggregate values;
- no escaping field references or interior mutation aliases; and
- equality only after its exact fieldwise/effect semantics are required and frozen.

Nested aggregates require independent depth/field/element limits.

## HIR-to-Core model

Nominal identity, construction, projection, and whole-value update remain explicit
and verified in typed HIR. The Phase-1 Core conversion is deliberately 1:N: one
fixed aggregate becomes its ordered scalar leaves, and aggregate block arguments,
calls, and returns become the same ordered leaf sequence. This matches Core's
existing scalar SSA and Minecraft realization model while keeping representation
out of source semantics.

Do not add a Core aggregate type merely to tear it apart immediately. Add one only
when a retained client needs aggregate identity to survive this conversion (for
example, a later compound materialization optimization that cannot be expressed by
leaf provenance). Conversion must retain a source-aggregate-to-leaf correlation for
diagnostics and artifact inspection.

Demand analysis should retain per-field uses so unused fields can be elided or avoid
materialization. Scalar replacement is an optimization over value semantics, not a
different source behavior.

## Physical model

Begin with:

1. fully scalarized score/frame field realizations;
2. elision of compile-time or wholly undemanded fields; then, only for a retained
   client, one typed NBT compound realization and explicit materialization.

Each compound field has a validated stable compiler-owned key. Recursive aggregate
arguments/results need typed frame layouts and copies that cannot alias an active
caller. The physical verifier reconstructs ownership, field completeness, type,
lifetime, and materialization relationships.

Do not choose one permanent representation from the source type alone.

## ABI

Freeze when a function parameter/result is:

- exploded into direct scalar fields;
- passed through activation-frame fields; or
- materialized in one NBT compound.

The initial policy may be deliberately simple and size-bounded. It must be
deterministic and inspectable, and adapters must be explicit. External exported ABI
stability is not implied by the internal policy during Phase 1.

## Required evidence

- missing/duplicate/unknown field diagnostics;
- construction and projection evaluation order;
- branch joins with whole values and field demand;
- copy independence after one source value is updated;
- calls, multiple results, recursion, and serial run children;
- scalarized versus compound-compatible policy differential when both exist;
- corruption tests for field realization and ABI mappings;
- absence of NBT aggregate support from scalar-only programs; and
- scale tests for wide/deep aggregates without dense cross-products.

## Design references

- [MLIR builtin tuple and 1:N conversion examples](https://mlir.llvm.org/docs/Dialects/Builtin/)
- [LLVM aggregate values, `extractvalue`, and `insertvalue`](https://llvm.org/docs/LangRef.html#aggregate-operations)
- [Rust MIR values, places, and projections](https://rustc-dev-guide.rust-lang.org/mir/index.html)
- [Cranelift frontend SSA variables](https://docs.wasmtime.dev/api/cranelift_frontend/index.html)

## Non-goals

- structural anonymous records unless separately selected;
- shared object identity;
- escaping references or borrowing;
- user-selected NBT layouts;
- reflection/dynamic field names; and
- general serialization derivation.
