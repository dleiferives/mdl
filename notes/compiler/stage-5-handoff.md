# Stage 5 to Stage 6 Handoff

Status: **Complete for the Phase 1 Java 26.2 vertical slice**

Stage 5 owns optional optimization, physical Minecraft planning, target-cost
analysis, artifact accounting, and their proof/reporting boundaries. Stage 6 may
orchestrate those boundaries behind a source-to-datapack façade; it must not merge
their ownership, diagnostics, or deterministic and empirical evidence into one
mutable compiler state.

## Proven producer boundaries

The completed pipeline has five independently owned outputs:

1. `optimize_core` consumes a `CoreProgram` and returns a verified
   `CoreOptimizationOutput { program, report }`. `None` is the unchanged verified
   reference; `Baseline` is the closed seven-step pipeline. Failure drops the
   possibly mutated unit and returns phase/function/step diagnostics plus one bounded
   snapshot.
2. `lower_to_minecraft` borrows verified Core and returns
   `LoweringOutput { program, map, report }`. The target program is verified, the ABI
   map is read-only, and the public opaque `LoweringDecisionReport` owns the frozen
   configuration, compact counts, physical decisions, recipe choices, and stable
   reason codes. Failures expose the frozen report only after plan publication.
3. `LoweringOutput::analyze_target_execution` is an explicit read-only analysis. It
   returns a separately owned `TargetExecutionCostReport`; analysis failure never
   discards an otherwise valid lowered target.
4. `emit_datapack` returns
   `EmissionOutput { pack, trace, footprint }`. `ArtifactFootprintReport` is exact
   deterministic evidence about the emitted artifact, not a lowering heuristic.
5. `mdl-test` owns versioned, self-validating `MeasurementRecord` JSONL values. Raw
   ordered timings, exact build/host identity, and optional Java/server identity for
   subjects that really launch Java are empirical evidence and never enter the four
   deterministic compiler reports.

Each successful output has borrowing accessors and an ownership-preserving
`into_parts` boundary where consumption is useful. Stage 6 may place these outputs
beside one another in a top-level compilation result, but it should retain their
concrete types and producer-specific failures.

## Optimization and reference contracts

- Core `None` and Minecraft `None` are permanent correctness/reference modes. No
  frontend or later stage may require canonicalized Core or Baseline physical
  planning for legality.
- The Baseline Core pipeline is closed and ordered: initial canonicalization, SCCP,
  DCE, straight-line fusion, dominance-scoped CSE, then the conditional final
  canonicalization/DCE cleanup pair.
- Target lowering remains one-way: semantic inventory, legality, runtime demand,
  optional sparse liveness, typed home assignment, edge transfers, closed control
  recipes, placement-aware resources, plan freeze, construction, reconciliation,
  and target verification.
- ABI homes remain fixed-slot, typed, single-context, and non-reentrant. Baseline
  coalescing only merges proven noninterfering same-type block-argument copies; the
  independent symbolic checker does not trust chooser liveness.
- The only Stage 5 control contraction is the proven zero-ABI terminal-call recipe.
  It preserves exact command success/result `1` on normal completion. The mandatory
  Stage 4 return dispatcher remains the fallback for every rejected or incomparable
  site.
- Recipe selection uses checked multi-dimensional local/whole-graph evidence; it
  never collapses runtime work, structured size, or artifact bytes into one guessed
  scalar weight.

## Measurement and testing boundary

Compiler-private observation hooks cover every Core step and lowering phase. A
release-only harness records its subject/configuration protocol explicitly and
counterbalances all four Core `None|Baseline` × Minecraft `None|Baseline`
configurations per subject after warm-up on tiny, normal, and scale fixtures. It
records raw samples for Core optimization, Minecraft lowering, target-cost analysis,
emission, and complete optimize-to-lower-to-emit compilation. The compiler-only suite
does not attach unused JVM metadata. There are no wall-time pass/fail thresholds;
deterministic visit/allocation counters remain the CI complexity gate.

Generated semantic/oracle tests retain a generator version, deterministic seed,
shape, and replay inputs on failure. Corruption tests cover editor batches, frozen
plans, placement/resources, assignments and symbolic homes, target cost graphs, and
predicted-versus-constructed recipe cost. Minimized failures should become named Rust
builder fixtures until Core has a round-trippable parser.

The one-startup Java 26.2 Core conformance test runs actual Core `None` and Baseline
optimization before the matching Minecraft policies, installs both outputs plus the
terminal-recipe differential, and checks branches, calls, loops, boundary integers,
parallel copies, real home coalescing, exact ABI/function outcomes, reload followed by
successful invocation, initialization collision handling, and attributable load
logs. `command_limits.rs` remains the separate authority for exact sequence/fork
boundaries; `minecraft_ir.rs` remains the direct structured-target gate.

Pinned Stage 5H server evidence:

```text
Minecraft server: official 26.2 JAR
Server SHA-256: cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5
Java: Homebrew OpenJDK 25.0.3
Data-pack format: [107, 1]
```

## Ownership after Stage 5

Stage 6 owns the minimal typed frontend, source spans, diagnostics, and top-level
source-to-datapack orchestration. It may call the existing optimizer, lowering,
analysis, and emission APIs; it should not move target facts into Core or expose a
public pass/recipe registry.

Stage 9 still owns soft tick budgets, static work partitioning, and any future
multi-tick runtime scheduler. The hard sequence/fork contract remains visible to it,
but Stage 5 does not schedule work.

Stage 11 owns inlining, specialization, outlining, region duplication, profile-guided
layout, global representation search, and equality saturation. Stable dual-guard or
snapshot branches, wider condition-stability analysis, and additional terminal
recipes remain deferred until a complete candidate wins under the shared accounting
and conformance process.

## Primary design references

- [MLIR pass instrumentation, statistics, timing, and verification](https://mlir.llvm.org/docs/PassManagement/)
- [MLIR testing and integration-test layering](https://mlir.llvm.org/getting_started/TestingGuide/)
- [Cranelift internal timing scopes](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/codegen/src/timing.rs)
- [Wasmtime generated differential testing and reproduction](https://github.com/bytecodealliance/wasmtime/blob/main/fuzz/README.md)
- [rustc profiling and self-profile tooling](https://rustc-dev-guide.rust-lang.org/profiling.html)
- [rustc testing layers](https://rustc-dev-guide.rust-lang.org/tests/intro.html)
- [GHC compiler dumps, timing, statistics, and verbose traces](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/debugging.html)
- [OCaml compiler profiling scopes](https://github.com/ocaml/ocaml/blob/trunk/utils/profile.ml)
- [OCaml Flambda statistics](https://github.com/ocaml/ocaml/blob/trunk/middle_end/flambda/inlining_stats.ml)
- [Zig release-build reproducibility and measured compiler evidence](https://ziglang.org/download/0.11.0/release-notes.html)

The complete research inventory and pass-by-pass rationale remain in
[`stage-5-baseline-optimization-plan.md`](stage-5-baseline-optimization-plan.md).
