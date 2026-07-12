# Stage 2 Typed SSA Implementation

Status: **Complete**

Stage 2 is implemented in `crates/mdl-compiler` as a dependency-free Rust crate.
The implementation follows the final design in
[`stage-2-ssa-plan.md`](stage-2-ssa-plan.md), including its deliberately deferred
features.

## Implemented substrate

- dense append-only typed entity stores with private ID construction;
- immutable UTF-8 source files, validated byte spans, line/byte-column lookup, and
  append-only unknown/source/call-site/fused provenance;
- compact `Bool` and `I32` Core value types;
- declare-then-define internal functions with stable ID identity and optional,
  non-unique name hints;
- ordered SSA bodies with block parameters, multi-result instructions, detached
  entity retention, and structural terminators;
- constants, wrapping and overflowing addition, signed comparisons, Boolean
  negation, and conservative internal calls;
- centralized operation names, type contracts, semantics, effects, and speculation;
- a body-scoped builder which derives result types rather than accepting them from
  callers;
- layered verification with accumulated structured diagnostics and fallible lookup;
- derived placement, use, CFG, DFS order, reachability, and Cooper-Harvey-Kennedy
  dominance analyses;
- canonical verified printing and a malformed-safe allocated-entity debug dump;
- in-place type/dominance/effect-safe replacement, erasure, terminator replacement,
  and unreachable-block detachment;
- named verify-after-pass execution with optional before capture, invalid after
  dumps, pipeline identity, and immediate failure stop.

## Proof corpus

Automated tests cover:

1. a value-producing diamond with an exact canonical snapshot;
2. nested Boolean branches;
3. loop-carried counter and accumulator block arguments;
4. overflowing arithmetic with two typed results;
5. forward and mutually recursive calls;
6. conservative call effects and rejected effectful erasure;
7. branch folding followed by unreachable cleanup;
8. rejection of a reachable cross-branch value leak;
9. source, call-site, and fused origins;
10. ordinary, edge, and return-use replacement;
11. dominance-invalid replacement with byte-identical rejection;
12. deterministic generated edit sequences;
13. explicit pass failure and invalid-output pass failure bundles;
14. malformed raw IR producing several diagnostics without panicking;
15. missing and duplicate function-definition lifecycle errors.

The analysis values borrow the body from which they were derived. Rust therefore
prevents mutable editing while an analysis view remains live without a cache revision
protocol.

## Intentionally still deferred

Stage 2 does not add a parser, rich or interned Core types, textual IR parsing,
physical ID compaction, analysis caching, general instruction motion, effectful
rewrites, Minecraft-specific effects, scheduling, e-graphs, or an external ABI.
Those remain assigned to later stages by the roadmap.

## Verification

The stage is accepted when these commands pass:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Core itself does not emit a datapack, so Stage 2 adds no vanilla-server test. The
existing Stage 1 harness remains the execution authority once Stage 3 introduces
datapack emission.
