# Stage 6 Implementation Checklist

Status: **Complete — Stages 6A–6I implemented, reviewed, and gated**

Completion evidence (2026-07-13):

- `cargo fmt --all -- --check`, strict workspace Clippy, the complete workspace test
  suite, rustdoc with warnings denied, Rust/Cargo 1.85.0 workspace checking, and
  `git diff --check` pass;
- `source_compilation_runs_all_four_policies_on_vanilla_26_2` passes all four
  Core/Minecraft `None|Baseline` combinations in one server startup; and
- `cli_materialized_pack_runs_on_vanilla_26_2` passes through the actual `mdl`
  process, materialized directory, printed ABI, and server invocation.

Both server gates used OpenJDK 25.0.3, Minecraft Java Edition 26.2 with data-pack
format 107.1, and server JAR SHA-256
`cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5`.

Design authority:
[`stage-6-minimal-frontend-plan.md`](stage-6-minimal-frontend-plan.md),
[`stage-6-modules-plan.md`](stage-6-modules-plan.md), and
[`stage-6-effects-plan.md`](stage-6-effects-plan.md)

Stage 6 adds the first typed source-to-datapack path while preserving the completed
Core optimizer, Minecraft lowering, target analysis, and emission boundaries.

## Rules for every work item

- [x] Keep syntax AST, resolved typed HIR, Core, target IR, and datapack emission as
  distinct verified layers; do not type the AST in place or bypass Core.
- [x] Leave the workspace compiling and tested at every section gate; land no
  `todo!`, `unimplemented!`, placeholder output, or deliberately panicking input path.
- [x] Preserve Core `None` and Minecraft `None` as complete correctness oracles.
- [x] Use existing option/output/failure types at the compilation façade; do not
  flatten producer errors or recompute reports.
- [x] Retain UTF-8 byte spans and `OriginId` provenance through every represented
  source construct and emitted physical line.
- [x] Allocate identities and produce diagnostics/dumps in source order. Hash maps
  may accelerate lookup but never determine observable order.
- [x] Bound source-size-derived recursion, recovery, diagnostics, and entity growth.
- [x] Add focused positive, negative, recovery, corruption, determinism, and scale
  tests with each feature.
- [x] Update the plan before implementation if evidence changes grammar, semantics,
  ownership, or a proof boundary.
- [x] Keep Stage 7 typed Minecraft APIs, Stage 9 scheduling, Stage 10 macros, Stage 11
  global optimization, and Stage 12 stable package/distribution policy out of the
  scalar frontend tranche.

## Stage 6A: Contract and shared diagnostics

- [x] **6A.1 — Audit the handoff, syntax ledger, and representable Core vocabulary.**

  Reconcile Stage 5 producer ownership with the Stage 6 roadmap. Record the exact
  first scalar grammar and explicit deferrals. Identify modules/imports, raw effects,
  overflow, visibility, and CLI boundaries rather than assigning them incidental
  parser behavior.

- [x] **6A.2 — Research mature frontend architectures.**

  Compare rustc, Zig, OCaml, GHC, MLIR/LLVM, and Cranelift using primary sources.
  Select AST → resolved typed HIR → Core, body-local dense IDs, signature-first name
  collection, a hand parser, structured flow checking, and direct loop-free SSA join
  construction. Reject unjustified query, dialect, lossless-CST, and generic pass
  infrastructure.

- [x] **6A.3 — Extend structured diagnostics compatibly.**

  Add an optional primary label message, ordered supporting labels, and attached
  notes. Preserve `Diagnostic::new`, `origin()`, existing equality, deterministic
  display, and all current verifier/lowering/emission behavior. Add focused API tests.

- [x] **6A.4 — Add pure source resolution and rendering helpers.**

  Slice validated spans, find source lines, resolve composite origins to a best
  source span, and render diagnostics without I/O. Test UTF-8, empty/end-of-file
  spans, call-site/fused origins, multiple labels, missing/unknown origins, and stable
  one-based user-facing line/columns.

- [x] **6A.5 — Close the 6A gate.**

  Review public compatibility and diagnostic output; run format, Clippy, tests,
  rustdoc, and MSRV checks before lexer work.

## Stage 6B: Lexer and recoverable AST parser

- [x] **6B.1 — Add the closed token vocabulary and bounded lexer.**

  Implement the documented identifiers, keywords, punctuation, decimal literals,
  whitespace, and `//` comments. Tokens retain `Span` only. Return all bounded lexical
  diagnostics with exactly one final truncation finding. Use `FrontendLimits` defaults
  of 1,000,000 tokens including EOF, nesting depth 256, and 100 ordinary diagnostics;
  consume invalid Unicode one scalar at a time. Test exact tokens/ranges, UTF-8,
  unknown characters, malformed arrows/operators, huge input, and EOF.

- [x] **6B.2 — Add the syntax-shaped internal AST.**

  Use ordinary Rust enums/structs without semantic dense IDs. Preserve complete construct
  spans. Retain explicit recovery nodes for statements/expressions where they aid
  synchronization; discard other malformed partial constructs at their recovery
  boundaries. Reserve `Option` for genuinely optional clean syntax.
  Do not expose a public mutable AST or copy source spellings into semantic identity.

- [x] **6B.3 — Implement the hand-written recoverable parser.**

  Parse functions, declarations, assignment, calls, conditionals, returns, prefix
  `!`, and one non-chaining comparison. Synchronize at the documented item,
  statement, and list boundaries; guarantee progress and enforce a nesting limit.

- [x] **6B.4 — Add deterministic AST dumps and parser gates.**

  Cover complete examples, trailing commas, empty lists/blocks, precedence,
  malformed delimiters, multiple independent errors, recovery termination,
  diagnostic caps, deep nesting, and repeated byte-identical dumps.

- [x] **6B.5 — Close the 6B gate.**

  Independently review the grammar implementation against the design plan, then run
  all fast completion commands.

## Stage 6C: Signatures, names, and typed HIR

- [x] **6C.1 — Add frontend-owned semantic types and IDs.**

  Define `ValueType`, `FunctionResult`, `SourceFunctionId`, owner-local `LocalId`, and
  immutable HIR nodes. References contain resolved IDs; every semantic node retains
  an origin. `Void` never appears as an expression value.

- [x] **6C.2 — Collect all function signatures before bodies.**

  Allocate functions in declaration order; reject duplicates with original-location
  support; validate parameter/result types; and enable forward calls and direct
  recursion. Treat every source function as externally callable in this stage.

- [x] **6C.3 — Resolve names and type-check while constructing HIR.**

  Allocate parameters/locals in source order; reject duplicate/shadowing names;
  enforce `const`/`var`; resolve direct calls; check arity, exact argument/result
  types, comparison types, `Bool` conditions, assignment, and return contracts.
  Preserve left-to-right argument traversal and suppress only dependent cascades.
  Parameters are immutable; declarations enter scope after their initializer; active
  shadowing is rejected while disjoint sibling scopes may reuse a spelling; branch
  locals expire at their closing brace. Permit a value call as a discard statement
  and reject a `Void` call in value context.

- [x] **6C.4 — Verify and deterministically dump successful HIR.**

  Add an internal verifier for identity ownership, references, types, mutability,
  signatures, and provenance. Add corruption tests and stable semantic dumps. No
  error sentinel or recovery node may survive success.

- [x] **6C.5 — Close the 6C gate.**

  Review diagnostics and phase invariants, then run all fast completion commands.

## Stage 6D: Flow checking and structured mutation

- [x] **6D.1 — Implement branch-sensitive definite assignment.**

  Track a dense assigned table with sparse checkpoint/rollback journals; intersect
  only newly assigned deltas from continuing arms; treat a missing `else` as the
  incoming state; and exclude terminating arms. Diagnose each actionable read with
  declaration support without emitting dependent type noise. Scale tests must reject
  whole-live-state cloning or scans per conditional.

- [x] **6D.2 — Implement reachability and return completeness.**

  Continue checking statements after unconditional termination for independent
  errors without allowing them to change continuation flow. Require all paths of a
  value-result function to return, permit `Void` fallthrough, and label result
  declarations/closing braces precisely.

- [x] **6D.3 — Add control-flow semantic gates.**

  Test nested branches, one/both/no continuing arms, early returns, uninitialized
  reads in conditions/call arguments/returns, assignment RHS-before-LHS behavior,
  `const` assignment, and deterministic multi-error ordering.

- [x] **6D.4 — Close the 6D gate.**

  Review the flow lattice and diagnostics, then run all fast completion commands.

## Stage 6E: Typed HIR to verified Core

- [x] **6E.1 — Predeclare source functions and retain correlation.**

  Convert exact source signatures to Core, declare in source order, and retain the
  typed source-function/Core-function map. Do not promise dense IDs as a stable ABI.

- [x] **6E.2 — Lower expressions, calls, and returns with provenance.**

  Map literals, resolved locals, `!`, comparisons, direct calls, and return values to
  the existing `FunctionBuilder`. Verify every operand/result type and preserve the
  HIR expression/statement origin.

- [x] **6E.3 — Lower mutable locals and `if` to deterministic SSA joins.**

  Maintain one journaled path environment, create branch/continuation blocks, omit
  terminated predecessors, pop branch-local bindings, and retain sorted net
  overrides before rolling back each arm. Aggregate only changed locals before
  merging differing values; no scratch pass may scan predecessors × candidates when
  no corresponding output exists. Definite assignment has already validated all
  later reads. Freeze all join parameters and complete edge arguments before
  installing predecessor terminators; order both by `LocalId`. A missing `else`
  carries the incoming environment explicitly. Charge actual join-edge operands to a
  checked whole-compilation cap derived from `FrontendLimits::max_tokens` before
  allocating parameters or edge vectors.

- [x] **6E.4 — Verify and dump the complete Core product.**

  Install bodies only after function verification, then verify the program. Add exact
  `None` Core goldens for forward calls, recursion structure, nested branches,
  assignment, early returns, and joins. Assert every Core entity's origin is valid.

- [x] **6E.5 — Close the 6E gate.**

  Independently review generated CFG/SSA, then run all fast completion commands.

## Stage 6F: Owned source-to-datapack façade

- [x] **6F.1 — Add owned input/options/frontend output types.**

  Accept one owned source name/text. Own `FrontendLimits` beside the existing Core optimization, Minecraft
  lowering, target-analysis limit, and emission options without copying their
  semantics. Retain sources, typed HIR, and function correlation on success/failure
  where diagnostics require them.

- [x] **6F.2 — Compose each existing producer exactly once.**

  Build Core, optimize, lower, analyze, and emit in order. Retain concrete outputs
  side by side. Store target-cost analysis as a nonfatal owned `Result`; never discard
  valid lowering/emission for instrumentation failure.
  Split temporary `CoreGenerationOutput { program, source_to_core }`, consume only its
  program in `optimize_core`, and retain the correlation map beside the optimized Core
  output without cloning the unoptimized program.

- [x] **6F.3 — Preserve exact failure ownership.**

  Use the concrete failure inventory in the design plan, including source ingestion,
  frontend infrastructure, syntax, semantics, Core generation, optimization,
  lowering, and emission. Retain all documented earlier products. Translate private
  lexer/parser/checker infrastructure errors into one precise public stage-aware
  owned type; do not misclassify them as source diagnostics. Use variants containing
  frontend diagnostics, `CoreOptimizationFailure`, `LoweringFailure`, or emission
  `Diagnostics`. Do not flatten them to strings or expose partial invalid IR. Add
  accessors and ownership tests.

- [x] **6F.4 — Add source entry/ABI convenience lookup.**

  Combine the source/Core correlation and borrowed `LoweringMap` to find each
  generated entry resource, ordered parameter homes, and result homes without
  duplicating either map.

- [x] **6F.5 — Close the 6F gate.**

  Review single-execution and ownership behavior, then run all fast completion
  commands.

## Stage 6G: Differential and vanilla vertical proof

- [x] **6G.1 — Add deterministic four-policy source fixtures.**

  Compile shared source programs under Core `None|Baseline` × Minecraft
  `None|Baseline`. Assert repeated byte equality where configurations match, exact
  maps, reports, traces, and source-origin continuity. Leave semantic equivalence to
  execution rather than claiming artifact comparison proves it.

- [x] **6G.2 — Add negative end-to-end source fixtures.**

  Prove malformed, unresolved, ill-typed, uninitialized, invalid-return, recursive
  target rejection, and invalid-option failures stop at the right ownership boundary
  with stable diagnostics. Test optimizer/emission ownership through narrow internal
  façade composition tests when ordinary valid source cannot naturally trigger those
  failures; do not add corruption syntax or public hooks.

- [x] **6G.3 — Run one pinned official-server proof.**

  Install and execute all four source-compiled Core/Minecraft policy combinations in
  one Java 26.2 startup. Invoke through generated mappings and assert parameters, calls, branches, mutable
  joins, `Bool`, `Int32`, `Void`, reload cleanliness, and reinvocation.

- [x] **6G.4 — Close the first source compiler gate.**

  Run all completion commands and record exact server/JDK/JAR/format evidence.

## Stage 6H: Minimal CLI process boundary

- [x] **6H.1 — Add a small compiler binary crate.**

  Read one source file, select explicit policies, invoke the library façade, render
  source diagnostics, and print generated source-entry mappings. Keep semantic
  filesystem discovery out of `mdl-compiler`.

- [x] **6H.2 — Write the complete in-memory artifact safely.**

  Preflight every validated relative pack path and refuse an existing nonempty output
  root before materializing deterministic files. Test in a temporary directory; do not add packaging,
  dependency resolution, installation, or release automation.

- [x] **6H.3 — Close the first human-usable gate.**

  Compile a checked-in source fixture through the binary, install its output in the
  existing server harness, run all completion commands, and document usage.

## Stage 6I: Multi-file and raw-effect contract resolution

The checked items in this section certify completed research, contract selection,
and roadmap reconciliation only. They do not mark any Stage 7A module or Stage 7B
effect implementation complete.

- [x] **6I.1 — Research and select module/import semantics.**

  The accepted module plan uses an explicit logical `ModulePath` independent of the
  diagnostic filename, module-only aliases and `alias::function`, separate
  private/package/export visibility, valid function-only import cycles, canonical
  module-order IDs, complete owned library input, and CLI-owned filesystem policy.

- [x] **6I.2 — Research and select the typed external-effect boundary.**

  The accepted effect plan uses a verified typed external Core operation, a closed
  compiler-owned intrinsic registry, and immutable, target-independently
  shape-checked fragment inventory owned by `CoreProgram`. Every initial external
  operation is `Unknown`, non-speculatable, and opaque; unsafe raw text receives no
  user effect-precision escape.

- [x] **6I.3 — Write separate implementation plans and reconcile ownership.**

  [`stage-6-modules-plan.md`](stage-6-modules-plan.md) assigns one-package
  whole-program modules to Stage 7A without dependencies or separate compilation.
  [`stage-6-effects-plan.md`](stage-6-effects-plan.md) assigns the external-operation
  spine, typed APIs, and static unsafe form to Stage 7B; scheduling consumption to
  Stage 9; and runtime templates/Minecraft macros to Stage 10.

- [x] **6I.4 — Reconcile the roadmap and Stage 7 handoff.**

  Stage 6 closes after its scalar/compiler/CLI gates and both reviewed 6I plans.
  Whole-package modules are the Stage 7A prerequisite; typed Minecraft effects are
  Stage 7B and later rather than untracked Stage 6 omissions.

## Completion commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo +1.85.0 check --workspace --all-targets
git diff --check
```
