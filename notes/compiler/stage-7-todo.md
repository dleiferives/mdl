# Stage 7 Implementation Checklist

Status: **complete and gated**

Authoritative design: [`stage-7-plan.md`](stage-7-plan.md). Audited remainder and
gate order: [`stage-7-remainder-plan.md`](stage-7-remainder-plan.md).

Implement these items in order. Each numbered tranche ends in a usable, verified
repository state. Do not begin broad Minecraft command coverage while an earlier
semantic boundary is incomplete.

## 7.0 — Freeze the remaining source policies

- [x] Use a Zig-like import declaration and dotted namespace model provisionally;
      exact spelling remains intentionally revisable.
- [x] Require possibly-many entity behavior to use an explicit fork such as
      `run.as(query) |entity| { ... }`; never auto-vectorize scalar methods.
- [x] Use a contextual `run.<modifier>(...)... { ... }` statement; preserve exact
      modifier order in one structured HIR region.
- [x] Optionally bind a known final executor with Zig-style `|executor|` capture;
      never inject a magic source variable named `self`. Omitting the capture does
      not change execution context or multiplicity.
- [x] Use a fluent typed entity-query plan with inferred kind/capability and
      cardinality facts; do not add raw selector strings, arbitrary predicates, or a
      separate query-comprehension grammar to the first slice.
- [x] Defer multi-version emulation and compatibility policy until a second target is
      an actual product goal.
- [x] Keep generated export resource names build-local through Stage 7; stable
      cross-datapack ABI names remain Stage 12.
- [x] Provisionally allow exports with inferred Minecraft context requirements and
      retain them in checked HIR. Publishing source and generated requirements remains
      an explicit completion item below.
- [x] Interpret `~` in direct `entity.teleport(~...)` against the current execution
      frame exactly as Minecraft does. Use explicit `entity.move_by(...)` or
      `run.as(entity).at_executor()` for receiver-relative movement.
- [x] Leave the existing untracked `notes/syntax/` work untouched; it is not
      compiler-owned input to this policy freeze.

Gate: accepted examples distinguish namespace values, captured executor methods,
explicit forks, unsafe raw commands, and target-invalid operation instances.

## 7A — Rooted module graph and namespace resolution

### 7A.1 — Inputs and deterministic normalization

- [x] Add validated `ModuleKey`, `ImportName`, `ModuleDependency`, `ModuleInput`, and
      rooted `PackageInput` ownership types.
- [x] Validate unique keys, one root, local dependency-name uniqueness, existing
      targets, and complete root reachability.
- [x] Canonicalize module/dependency traversal before allocating file, origin,
      module, or function IDs.
- [x] Add package-wide module/token/diagnostic limits and exhaustion diagnostics.
- [x] Turn `compile_source` into a root-only `compile_package` adapter.
- [x] Prove input permutation does not change diagnostics, IDs, reports, traces, or
      emitted files.

### 7A.2 — Postfix syntax and recovery

- [x] Add the selected import form and string support required by it.
- [x] Parse a uniform postfix member/call expression instead of a special `::` call.
- [x] Preserve component/member/call spans independently for diagnostics.
- [x] Recover without swallowing the next import, visibility modifier, function, or
      statement.
- [x] Update deterministic token and AST dumps.

### 7A.3 — Package index and namespaces

- [x] Build the full reachable module/function signature index before checking any
      body.
- [x] Bind each import name through the owning module's dependency map.
- [x] Represent namespaces only in the resolver; lower resolved calls directly to
      `SourceFunctionId`.
- [x] Enforce private/public/export visibility with cross-file supporting labels.
- [x] Accept dependency and call cycles at the frontend while preserving the typed
      backend recursion rejection where still required.
- [x] Poison invalid bindings to prevent cascaded diagnostics.

### 7A.4 — HIR/Core linkage and exports

- [x] Add owning-module identity and visibility to HIR functions and dumps.
- [x] Add verified `Internal | DatapackExport` Core linkage.
- [x] Keep exports live as unknown external roots through optimization.
- [x] Distinguish internal generated functions from supported datapack entries in
      lowering, cost reports, and facade maps.
- [x] Preserve exact source/module/function/Core/target provenance.

### 7A.5 — Vertical and scale gate

- [x] Cover aliases, forward references, visibility, duplicate local names,
      dependency cycles, unresolved/private members, unreachable input, and limits.
- [x] Cover namespace/method parse ambiguity before methods are introduced.
- [x] Run an exported cross-module call on the pinned Java 26.2 server under every
      optimization/physical-policy combination.

Gate: all scalar Stage 6 programs remain byte-stable through the adapter where the
new export spelling does not intentionally change their public entry map.

## 7B — External operation substrate and literal unsafe command

### 7B.1 — Linked Core inventories

- [x] Add `ExternalOpId`, `TargetFragmentId`, closed external declarations, immutable
      fragment ownership, and `CoreOp::External`.
- [x] Update builders, dumps, verifier, batch editor, reachability, cloning, every
      exhaustive optimization match, lowering audits, and corruption tests.
- [x] Enforce ordered typed operands/results and `Unknown/Never/Opaque` centrally.
- [x] Add one closed function-reference enumerator covering ordinary calls and
      future call-like external bindings; route every current non-SSA edge consumer
      (semantic inventory, call graphs, and SCC/recursion checks) through it. The
      first call-like run-scope binding must add its 7C verifier/cloning/behavior
      tests against the same hook.
- [x] Prove DCE, CSE, canonicalization, block fusion, and SCCP cannot erase, merge,
      move, inspect, or speculate external behavior.
- [x] Add deterministic allocation and scale tests; avoid dense cross-product state.

### 7B.2 — HIR boundary

- [x] Add HIR external/unsafe nodes that store semantic identity and source data but
      do not copy registry contracts or target builders.
- [x] Verify the literal-unsafe HIR node through the shared compiler-owned physical
      command-line validator. Typed semantic nodes will use the Stage 7D descriptor
      registry once that registry exists.
- [x] Lower HIR external nodes into program-owned declarations/fragments in canonical
      source order.
- [x] Preserve origin chains through optimization and target construction.

### 7B.3 — Literal unsafe syntax and validation

- [x] Add the chosen string literal representation and
      `unsafe minecraft("...");` statement.
- [x] Separate target-independent physical-line validation from target-specific
      UTF-16/length validation.
- [x] Reject interpolation, concatenation, values, and user effect claims.
- [x] Lower exactly one validated fragment through the existing target unsafe node;
      never reclassify text as a safe command. Isolate each fragment in a dedicated
      helper so opaque `return` behavior cannot depend on compiler block partitioning.
- [x] Report local emitted line count but transitive work/forks/context as unknown.
- [x] Test compiler rejection separately from expected real-server Brigadier reload
      rejection.

Gate: the literal unsafe vertical slice passes deterministic frontend/Core/target
differentials and the pinned server without weakening any optimizer.

## 7C — Close the implemented query/run-scope substrate

The broad semantic types, query cardinality, `.as` scope, HIR behavior solver, Core
run-scope operation, fixed-slot gate, and target lowering are implemented. Stage 7C
closes their evidence and successful-contract retention. The Core ambient analysis
lands in 7D, where `Say` first supplies an exact executor requirement.

- [x] Add nested fixtures for inherited zero-modifier context, inner `.as` context
      replacement, exact modifier/query order, and source-to-pack provenance.
- [x] Return immutable `CommandLimitEvidence` from legality instead of discarding
      successful evidence; Gate 7D later combines it with recipe selections into
      `TargetPreflight`.
- [x] Retain the reachable strict minimum `minecraft:max_command_forks` and thread it
      through the plan, verifier, report, map, deployment contract, dumps, and CLI.
- [x] Distinguish configured assumptions, target defaults, and derived deployment
      minimums in every public report.
- [x] Print exported `FunctionBehavior` in the CLI source-ABI report.
- [x] Add corruption, recursion, zero/repeated/nested-prefix, unreachable-scope,
      insufficient-assumption, determinism, and scale tests for these additions.

Gate: nested `.as` structure/provenance is proven source-to-target, and a successful
build publishes the minimum fork setting it actually requires.

## 7D — First typed Minecraft operation

### 7D.1 — Freeze `Say` meaning before syntax resolution

- [x] Complete an owned `MessageLiteral` attribute. Initially reject empty text,
      control characters, and `@` selector interpolation; measure whitespace, quote,
      backslash, and Unicode preservation on the pinned server. Reject a rendered
      terminal backslash until a non-continuation escaping recipe is proven.
- [x] Turn the pinned Java 26.2 bytecode observation of a 256 UTF-16 code-unit message
      limit into a recipe test, separate from target-independent message validity and
      the complete rendered-command limit.
- [x] Add `ObservableEffect = None | Observable | Unknown` independently from world
      state effects; classify `Say` as observable without claiming a world write.
- [x] Freeze and record Java 26.2 `say` success/result behavior with a server
      differential even though the source result is `Void`; keep this native result
      distinct from the target-cost solver's `CommandOutcome::Continue`.

### 7D.2 — Closed semantic registry and source method

- [x] Add `MinecraftSemanticKey::Say` and one complete target-independent descriptor
      for signature, ambient context, effects, fork/work, outcome, validation, and
      documentation.
- [x] Keep source receiver normalization separate: captured
      `Executor<T: CommandExecutor>.say(MessageLiteral)` resolves to ambient `Say`.
- [x] Reject stale/outer captures, ordinary values, wrong arity, nonliteral messages,
      expression use of `Void`, and unknown methods with precise origins.
- [x] Store the exact lexical executor proof only in HIR and make HIR verification
      replay the current context.
- [x] Drive method lookup, HIR verification, behavior inference, and support docs from
      exhaustive consumers of the closed registry. Allow handwritten irregular
      validators without an extension map or schema DSL.

### 7D.3 — Core normalization and independent context proof

- [x] Add a program-owned Minecraft semantic-operation declaration and
      `ExternalSemanticBinding::MinecraftOperation(id)`.
- [x] Lower `Say` with closed attributes, instantiated receiver kind, and provenance,
      but erase the already-checked lexical proof rather than fabricating an SSA
      executor or retaining a source-scope ID across outlining.
- [x] Make the Core ambient solver treat `Say` as requiring its executor kind and
      analyze only CFG-entry-reachable operations. Prove propagation through calls,
      nested zero-modifier scopes, `.as` discharge, recursion, unsafe unknowns, and
      malformed receiver metadata; defer cross-kind tests until a second kind exists.
- [x] Differentially compare independently inferred HIR and unoptimized Core entry
      requirements. Retain optimized requirements in one independently verified dense
      `CoreAmbientAnalysis`; do not copy them into Core declarations.
- [x] Add iterative SCC corruption/determinism and 20,000-function chain/fanout/cycle
      scale tests without recursive host traversal.
- [x] Keep the Core operation `Unknown/Never/Opaque` to optimizers and update every
      printer, verifier, editor, reachability path, exhaustive pass match, and
      corruption test.

### 7D.4 — Retained target recipe and structured target IR

- [x] Add instance-aware `MinecraftRecipeId::Java26_2Say` selection during preflight
      and combine it with retained `CommandLimitEvidence` in `TargetPreflight`; use a
      dense optional recipe slot per external declaration.
- [x] Independently verify the standalone preflight's target, operation instances,
      reachability, dense slots, and recipes; do not construct partial output on
      failure.
- [x] Keep the current one-key/one-target product total. Do not invent a fake target
      only to test an unsupported-pair diagnostic.
- [x] Keep exact compile-target facts and the required conformance Java runtime
      version in `TargetSpec`; keep the resolved Java executable, pinned server
      JAR/hash, and measurement evidence in the conformance harness.
- [x] Add structured `Say` target IR and update its verifier, renderer, context/effect
      census, local/global cost and outcome analyses, contract, and exhaustive matches.
      Add a known `OUTPUT`/communication effect category instead of calling output a
      world write or leaving it effect-free.
- [x] Add one authoritative target-local native success/result/continuation contract
      consumed by recipe reconciliation and the global solver; do not confuse native
      result `Exact(1)` with cost-control `CommandOutcome::Continue`. Reconciliation
      checks the exact value; the existing solver consumes its `NonZero` projection
      where a result is observed, without widening the solver domain.
- [x] Emit typed `Say` directly inside the containing generated function while raw
      text remains isolated. Retain the current verified run-scope helper/outlining
      scheme for Stage 7.
- [x] Update assignment, conditional helper allocation, plan assembly, plan
      verification/reporting/symbolic checks, and emission together; preflight must
      reach resource allocation so direct `Say` receives no helper.
- [x] Borrow standalone `TargetPreflight` and `CoreAmbientAnalysis` during resource
      allocation, then move both into the first correct `Say`-capable `LoweringPlan`.
      Plan verification recomputes/validates them; reports contain projections only.
- [x] Publish optimized Core entry requirements per `LoweredFunction` and function
      report, not in the pack-wide `ExecutionContract`.
- [x] Reconcile descriptor/recipe predictions with the constructed target command and
      assert typed `Say` never crosses `UnsafeRawCommand`/`TargetFragment`.

### 7D.5 — Reports, differential proof, and real server

- [x] Correlate HIR occurrence, Core declaration, selected recipe, generated command,
      physical placement/cost, and exact provenance in deterministic bounded reports.
      Label HIR source-semantic requirements separately from optimized-Core generated
      entry requirements.
- [x] Capture a post-construction map from each `(Core FunctionId, InstId)` semantic
      occurrence to its actual target function/command IDs. Preconstruction failure
      reports contain only planned recipe/placement/resource identities.
- [x] Generate the small supported source-API/target matrix from method and recipe
      data.
- [x] Add valid, invalid, empty-match, unbounded-query, stale-capture, message-policy,
      target-length, corruption, and four-policy deterministic source fixtures.
- [x] On one pinned Java 26.2 server start with no client, summon a tagged armor stand
      in a force-loaded chunk, prove executor-attributed `say`, prove empty-match skip,
      and retain the unsafe-command regression. Measure native result separately with
      handwritten `execute store result ... run say ...`; the compiled `Void` wrapper
      exposes the function result, not the nested command result.
- [x] Audit the pinned `commands.json` shape `say -> message: minecraft:message`; do
      not treat that syntax tree as effect, context, or outcome evidence.

Gate: one captured `Executor.say` program compiles to structured Minecraft IR and a
deterministic datapack, with independently verified context/multiplicity and measured
Java 26.2 behavior.

## Explicitly deferred beyond the Stage 7 exit

- [x] Defer query-to-`EntityRef` strengthening and a physical entity-handle
      representation until an absence/multiplicity contract and Stage 8 value bridge
      require them.
- [x] Defer `EntityRef.say` implementation; document it as future source sugar for an
      exact-one `.as` scope plus the same ambient `Say` operation.
- [x] Defer coordinates and `at`, `in`, `rotated`, `positioned`, `facing`, `anchored`,
      `align`, relative/local frames, `store`, conditions, `on`, and `summon`.
- [x] Defer general caller-imported source executor capabilities, possibly-many method
      syntax, broad command coverage, a second target, emulation, and compatibility
      policy.
- [x] Add semantic signatures/value representations only when a concrete operation
      consumes them; add no type interner without recursive structure or measurement.

## Completion audit

- [x] Run `cargo fmt --all --check`.
- [x] Run `cargo clippy --workspace --all-targets -- -D warnings`.
- [x] Run `cargo test --workspace`.
- [x] Run warnings-denied rustdoc and the pinned MSRV gate.
- [x] Run Stage 7 scale, generated differential, and official-server suites.
- [x] Review every new exhaustive match and verifier corruption case independently.
- [x] Check deterministic outputs across input permutations and repeat builds.
- [x] Update compiler README, roadmap, handoff, semantic ambiguities, and references.
- [x] Record all unresolved Minecraft behavior as explicit compiler ambiguity notes,
      never as optimistic contracts.
