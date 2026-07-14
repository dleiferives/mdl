# Stage 7B: Typed Minecraft Effects and the Unsafe Raw Boundary

Status: **Accepted Stage 6I contract; implementation assigned to Stages 7B–10**

Stage 6I resolves how Minecraft behavior enters the verified compiler pipeline. It
does not implement that behavior. The selected design has two deliberately unequal
paths:

1. compiler-owned typed Minecraft operations with checked source types, semantic
   descriptors, and structured target lowering; and
2. an explicit unsafe raw-command escape hatch whose contract is permanently
   conservative unless the compiler itself understands the command.

Both paths become ordinary ordered Core operations before optimization. Neither path
may mutate `MinecraftProgram` from the parser/HIR, append emitted text behind the
lowerer's back, or rely on a cleanup pass to become legal.

## Why the boundary belongs after Stage 6

The scalar Stage 6 language has only `Bool`, `Int32`, and `Void`. Correct typed
Minecraft APIs need entity/cardinality types, execution context, locations, text,
command success versus result, storage/NBT types, and target capabilities. Runtime
insertion into command syntax additionally needs Minecraft function-macro
serialization and cost semantics. Pretending typed interpolation is complete before
those types exist would make the escape hatch less typed than its design promise.

Roadmap ownership is therefore explicit:

- **Stage 7B** implements the Core external-operation spine, the first compiler-owned
  typed Minecraft APIs, and a literal-only unsafe raw statement;
- **Stage 8** selects physical representations for new semantic value types and
  conversions across the external boundary;
- **Stage 9** consumes exact or conservative context/fork/work contracts when it
  schedules and must treat unsafe raw commands as indivisible unknown work;
- **Stage 10** adds typed runtime interpolation and the Minecraft function-macro ABI,
  separately from language macros; and
- **Stage 11** may exploit more precise compiler-owned effects only after analysis,
  cost, and server evidence justify it.

This is a reviewed reassignment of Stage 6's former “raw-command escape hatch with
typed interpolation” bullet. Stage 6 finishes its scalar/compiler/CLI gates and both
6I plans; neither whole-package modules nor effects implementation is a hidden Stage
6 completion condition.

## Research consequences

Three mature-compiler patterns matter directly:

- MLIR models implicit behavior and speculation separately. An operation with
  effects cannot be reordered, eliminated, or introduced merely because its SSA
  operands match, and speculatability is a distinct question.
- LLVM inline assembly retains typed operands/constraints and explicit flags, and
  tells correctness-critical analyses not to infer semantics by inspecting its
  template text.
- Rust makes claims such as `pure`, `nomem`, and `readonly` unsafe semantic promises;
  those claims grant the optimizer observable freedoms and are checked for required
  combinations.

MDL adopts the conservative lesson, not the annotation surface. Phase 1 users cannot
claim that arbitrary Minecraft text is pure, read-only, non-forking, deterministic,
or context-independent. Compiler-owned operations may receive stronger contracts
because their implementation, verifier, target lowering, and conformance tests are
all owned together.

## Selected semantic split

### Compiler-owned typed operations

Safe source APIs are normal typed calls in the spirit of:

```text
minecraft::tp(player, destination);
player.say(text);
const outcome: CommandSuccess = minecraft::try_set_block(location, block);
```

These spellings do not resolve to library functions containing raw text. Name and
method resolution select a closed compiler semantic key such as
`MinecraftIntrinsic::Teleport` or `MinecraftIntrinsic::Say`. The registry is Rust
code owned by the compiler and target profiles; there is no source-level
`extern effect`, user operation registry, or plug-in callback in the first tranche.

`minecraft::` is a reserved compiler-intrinsic namespace, not an implicitly imported
source module and not a filesystem lookup. Stage 7A reserves that leading module-path
component and the `minecraft` import alias so package input cannot shadow it. Method
syntax such as `player.say(text)` resolves through the receiver type to the same
registry. A later standard library may wrap intrinsics, but wrappers do not replace
their semantic keys with raw text.

Each intrinsic declaration defines:

```text
MinecraftIntrinsicDescriptor {
  semantic_key
  source_operand_types
  source_result_types
  required_context
  context_transition
  cardinality_or_fork_contract
  command_outcome_contract
  target_capabilities
  structured_lowering_key
}
```

Every runtime value is an explicit typed operand or result. Static semantic choices
such as an enum variant may be a verified descriptor attribute. Ambient Minecraft
context is never guessed from a string: Stage 7's context checker proves the
descriptor's requirements at the call site, and the descriptor carries the same
contract into lowering. An operation that transforms context or forks cannot present
itself as an ordinary scalar call merely because it eventually emits one line; the
Stage 7 context plan must represent that transition explicitly.

Stage 7B implements only intrinsics whose operands and results have one documented,
correct canonical Core representation. The translation from the descriptor's source
semantic signature to the actual `ExternalOpDecl` Core signature is explicit and
verified; it never erases a value to raw text. Stage 8 turns that single-choice
bridge into representation selection with alternatives and explicit conversion
costs. Thus the Stage 7 vertical slice is executable without making its provisional
physical choice part of source semantics.

`CommandSuccess` and `CommandResult` are distinct source semantics. Neither is
silently interchangeable with `Bool` or `Int32`; explicit conversions and their
failure/absence behavior are required. `Void` produces no SSA result. Arguments are
evaluated left to right before the effect executes.

Safe intrinsics lower through typed `CommandKind`, `ExecuteModifier`, selector,
score, storage, NBT, and future structured target constructors. They never construct
`UnsafeRawCommand` as a shortcut. If the structured Minecraft IR lacks a required
shape, that target IR is extended and independently verified first.

### Unsafe raw commands

The initial escape hatch is visibly unsafe, literal-only, statement-only, and
target-specific:

```text
unsafe minecraft("say hello");
```

The exact surface spelling lands with Stage 7's string-literal lexer work, but these
semantics are fixed:

- the argument is one compile-time source literal, not a runtime `String`;
- interpolation and concatenated command construction are rejected;
- the statement produces no source value and discards Minecraft success/result;
- source/HIR validates target-independent physical-line shape, while the selected
  target later validates target-specific length and representation limits;
- Brigadier syntax and runtime meaning remain the real server's authority; and
- the operation is an unknown effect, context, outcome, and fork barrier.

The existing target `UnsafeRawCommand` validator remains the final selected-target
boundary. Its current validation is split when this tranche lands: a target-neutral
fragment validator diagnoses an empty line, CR/LF, boundary whitespace, reserved
`/`, `#`, or `$` prefix, and terminal continuation with the responsible literal
origin; target lowering enforces Java UTF-16 length and any other profile-specific
physical rule. A structurally safe but unknown Minecraft command can still fail at
datapack reload; `unsafe` is honest about that limit.

There is no user-written effect annotation in this tranche. In particular these are
not accepted:

```text
unsafe minecraft("...", pure = true);
unsafe minecraft("...", forks = 1);
unsafe minecraft("...", reads = []);
```

If a command becomes common enough to need optimization or scheduling guarantees,
the correct first response is a compiler-owned typed intrinsic with tests. A later
unsafe deployment assertion would need its own explicit unsoundness contract; it is
not smuggled into this design.

### HIR boundary

Successful typed HIR keeps the two forms explicit:

```text
HirExternalOperation {
  intrinsic: MinecraftIntrinsicKey
  operands: [HirExpression]
  result_types
  attributes
  context_contract
  origin
}

HirUnsafeMinecraftLine {
  literal
  literal_origin
  statement_origin
}
```

The checker constructs the first form only after ordinary overload/method resolution,
exact operand/result typing, and context/cardinality validation. The unsafe form is a
statement and cannot masquerade as a normal `String` call. Neither HIR node contains
a `CommandNode`, target builder, renderer callback, or mutable output handle. HIR
verification repeats registry/signature/context consistency before Core lowering.

## Core representation

Core remains closed and typed. Add an external-operation identity and a verified
declaration table rather than embedding target text in each instruction:

```text
ExternalOpId
TargetFragmentId

CoreOp
  ...
  External(ExternalOpId)

CoreProgram {
  functions
  external_operations: EntityVec<ExternalOpId, ExternalOpDecl>
  target_fragments: EntityVec<TargetFragmentId, UnsafeTargetFragment>
}

UnsafeTargetFragment {
  decoded_line
  origin
}

ExternalOpDecl {
  diagnostic_name
  parameter_types
  result_types
  semantic_binding
  attributes
  origin
}

ExternalOpAttributes                    // closed, key-specific typed attributes

ExternalSemanticBinding
  MinecraftIntrinsic(MinecraftIntrinsicKey)
  UnsafeTargetFragment(TargetFragmentId)
```

`CoreOp::External` stores only the typed declaration ID. Compiler-owned intrinsic
semantics live in the closed registry. `CoreProgram` owns both immutable declaration
and target-fragment inventories, just as it already owns function declarations and
bodies. Unsafe literal payloads are target-neutrally shape-checked fragment records,
not strings on instructions, and generic optimization never inspects their text.
Keeping the inventories inside the consumed verified unit prevents detachment while
preserving the existing `optimize_core(CoreProgram)` and
`lower_to_minecraft(&CoreProgram, ...)` ownership boundaries; no parallel
`CoreGenerationProduct` wrapper is introduced.

Core generation remains target-independent. In particular, the existing `lower_hir`
boundary (including its target-independent source-derived resource budget) does not
gain target lowering options or a selected Minecraft target. An
`UnsafeTargetFragment` proves only target-independent
source/physical-shape invariants; it is not an `UnsafeRawCommand` and does not claim
that any target accepts its physical representation. Minecraft lowering resolves the
selected profile, performs the remaining target checks, and only then constructs the
target IR raw node.

This table is a narrow linked semantic inventory, not a general dialect registry,
attribute map, dynamic trait object, plug-in ABI, or serialization format. IDs are
dense and compilation-local, allocated in canonical module/function/source order.
Equal literal payloads need not be interned in the first implementation; observable
ordering must not depend on a hash table.

`attributes` is a closed typed enum for compiler-understood static choices, not a
string-keyed bag. The intrinsic key determines its permitted attribute variant. All
ordinary runtime inputs remain SSA operands; an attribute cannot hide a value whose
evaluation order or representation is observable.

The alternative of representing every external operation as an undefined ordinary
Core function was rejected. It would reuse `core.call`, but it would also force
Minecraft context transitions, intrinsic capability requirements, and raw fragment
linkage into a function ABI that currently means an internal definition with a
fixed-slot calling convention. An explicit operation makes the different verifier
and lowering obligations visible.

### Core verifier obligations

For every external instruction, independent verification checks:

- `ExternalOpId` exists in the same `CoreProgram`;
- operand and result arity/types exactly match the declaration;
- every descriptor semantic key exists in the closed compiler registry;
- each declaration's typed attribute variant is legal for that semantic key;
- descriptor source/Core signatures agree after explicit type conversion;
- target-fragment IDs exist and are used only by unsafe bindings;
- unsafe bindings have no result and the fixed conservative contract;
- unsafe fragments satisfy the target-independent structural invariants;
- origin IDs for the declaration, instruction, fragment, and template segments are
  valid; and
- intrinsic keys refer to descriptors whose capability requirements are closed
  compiler-owned identities, not arbitrary string features.

Corruption tests independently break each link, signature, contract, fragment, and
origin. The Core printer emits a stable symbolic external key plus exact types and a
fragment ID; it does not print an unsafe command as if it were ordinary Core
semantics. A separate explicitly unsafe diagnostic dump may show escaped source text.

### Coarse optimization contract

The first implementation uses the current three orthogonal Core queries exactly:

```text
effects()            = EffectClass::Unknown
speculation()        = Speculation::Never
result_equivalence() = ResultEquivalence::Opaque
```

This applies to every `CoreOp::External`, including compiler-owned typed Minecraft
operations. It guarantees:

- DCE cannot remove an unused external operation;
- CSE cannot combine two structurally equal instances;
- canonicalization cannot commute operands or move the operation;
- a pass cannot hoist it out of its original control dependence;
- duplication requires a later operation-specific proof; and
- results are not assumed equal across calls with equal operands because world state
  and execution context can change.

The rule is conservative but proportional. Current Core has only `Pure|Unknown`, and
the existing optimizer already treats calls as `Unknown/Never/Opaque`. Do not add a
place-level effect/alias lattice until a concrete pass consumes it. Compiler-owned
descriptors and structured target commands retain their richer Minecraft
`CommandContract` for lowering, target optimization, cost analysis, and Stage 9.
Unsafe raw commands remain unknown at every level.

If Stage 11 later teaches Core about precise external reads/writes, precision is
derived from the closed intrinsic key, never copied from source annotations. Effect,
speculation, and result equivalence remain separate; “known read” does not imply
safe speculation or equal repeated results.

## Target versions and capabilities

Target support is a closed compiler fact. Add named capability identities only when
an intrinsic or lowering consumes them, for example a future
`MinecraftCapability::FunctionMacros`. `TargetSpec` answers those closed queries by
matching the selected `JavaEditionTarget`; callers cannot construct an arbitrary bag
of `supports_x` booleans.

An intrinsic names semantic requirements, not a guessed minimum-version string. The
target lowerer either selects a verified recipe or returns a typed
`UnsupportedExternalOperation` containing the intrinsic, target, required
capability, and source origin. Raw fragments carry no portability promise: each
Minecraft lowering validates the fragment against its selected target before
constructing `UnsafeRawCommand`.

The Phase 1 target remains Java 26.2/data-pack format 107.1. Adding a newer profile
does not silently bless or reinterpret an older raw command. Capability-selected
structured command forms, pack paths, macro availability, length bounds, and
serialization facts stay behind the target profile and target IR; spelling inside an
unsafe fragment remains the user's explicitly nonportable responsibility.

## Structured target lowering and validation

Lowering one external operation follows this order:

1. resolve its verified `ExternalOpDecl` and the selected target;
2. branch on its verified semantic binding:
   - for an intrinsic, check its descriptor's closed capability requirements, map
     its already-represented SSA operands to verified target homes, select a
     verified recipe with exact context/fork/outcome behavior, and emit structured
     Minecraft IR; or
   - for an unsafe binding, require its fixed no-result signature, validate its
     fragment against the target profile, and construct exactly one
     `UnsafeRawCommand` node;
3. reconcile predicted commands/cost with the constructed program; and
4. run the ordinary Minecraft verifier and emitter.

The lowering report records the external semantic key, selected recipe or raw
barrier reason, generated command IDs, and provenance. A target recipe may call
helper functions or use multiple commands; source “one operation” does not promise
one output line.

Compiler-owned operation validation happens at the most semantic layer available:

- source/HIR validates source types, cardinality, required execution context, and
  target-independent unsafe-fragment shape;
- Core validates SSA types, declaration linkage, fragment linkage/shape, and the
  ordering contract without consulting a target;
- target lowering validates capabilities, target-specific raw-command length, and
  then uses target constructors for resource IDs, selectors, NBT, score shapes, and
  command structure;
- emission rechecks final physical paths and target IR invariants; and
- the pinned vanilla server validates real target behavior.

No layer reparses rendered text to recover structure it previously discarded.

### Failure ownership

Source operand/type/context errors and target-independent unsafe-literal shape
hazards are ordinary semantic diagnostics retaining `SourceContext`. External/table
identity exhaustion and a verifier-rejected HIR/Core product remain structured
infrastructure or Core-generation failures. Unsupported target capabilities,
target-specific raw-command rejection (including length overflow), and failed
structured target construction are typed Minecraft lowering failures that retain the
checked frontend, source/Core map, and optimized Core under the established façade
contract.

An unsafe line that passes both structural and selected-target validation but is
rejected by Brigadier is not a compiler diagnostic: compilation intentionally
succeeds, and datapack reload on the real server reports the unsafe target failure.
Server conformance tests distinguish that expected unsafe behavior from
compiler/emitter regressions. Stage 10 template shape, placeholder, and serializer
errors are compile-time diagnostics; a runtime substitution value that makes an
otherwise unsafe template invalid follows the documented Minecraft macro failure
semantics.

## Typed interpolation and Minecraft function macros

Runtime interpolation is Stage 10 because Minecraft substitutes macro variables into
command syntax at invocation time. It is not ordinary string formatting. The target
requires a compound containing every referenced variable, reparses substituted
lines, may skip an entire call after a syntax error, and has parameter-set-dependent
parse/cache costs.

Stage 10 extends the unsafe form with a parsed template representation, not free-form
concatenation. A template owns ordered literal and placeholder segments:

```text
CommandTemplate {
  segments: [Literal | Placeholder(TemplateOperandId)]
  operands: [TemplateOperand { name, source_type, serializer }]
  origin
}
```

Validation checks placeholder spelling, uniqueness, exact operand coverage, source
types, serializer availability, literal physical hazards, maximum possible static
length where provable, and target macro capability. Each permitted source type has a
compiler-owned target serializer with explicit escaping, range, and NBT/command-token
rules. An ordinary runtime `String` is never accepted as trusted command syntax.

The target IR gains a distinct validated macro-line node and typed macro-call
arguments. It does not weaken `UnsafeRawCommand::new`, whose leading `$` rejection is
still correct for non-macro lines. Lowering may replace a runtime macro with constant
specialization or dispatch, but both alternatives implement the same typed template
semantics and remain visible to cost reporting.

Language macros remain a different Stage 10 mechanism. They expand typed or
type-checkable language constructs before normal HIR/Core verification and do not
automatically emit Minecraft macros.

## Optimization, cost, and scheduling consequences

### Core and target optimization

- The Core optimizer preserves external instruction order and control dependence.
- Block/CFG transformations remain legal only when they preserve the sequence of
  `Unknown/Never/Opaque` operations on every path.
- Typed Minecraft operations may participate in target-level context-prefix sharing
  only through their structured command contracts and an explicit proof.
- Raw commands block context fusion, command duplication, condition re-evaluation,
  and effect-sensitive motion across the barrier.
- Line count is recorded but never substitutes for runtime work, fork, macro parse,
  or generated-size accounting.

### Target cost analysis

Compiler-owned structured lowerings contribute their exact command recipes and
known command contracts. A literal raw command is one emitted physical line but may
call functions, fork through selectors, change context, or perform unknown nested
work. Its local line count is therefore known while its transitive sequence/fork
summary remains `Unknown`.

Stage 10 macro recipes additionally account for runtime parsing and parameter-set
cache behavior. Repeated identical parameters and novel parameters are distinct
measurement subjects; neither receives a fabricated universal coefficient.

### Stage 9 scheduling

Static scheduling may split only at compiler-known continuation boundaries. It may
not yield inside, duplicate, or replay an external operation unless the closed
intrinsic contract proves that transformation. Unsafe raw commands are indivisible
unknown roots. If their fork or transitive work prevents a hard command-limit proof,
the scheduler returns an honest unknown/unbounded diagnostic rather than accepting a
soft per-tick guess.

Execution context cannot be assumed to survive a scheduled yield. Stage 7 context
requirements and transformations become explicit scheduler live-state obligations;
raw context is unknown and therefore cannot be reconstructed automatically.

## Implementation tranches

### Stage 7B.1 — External Core substrate

- add `ExternalOpId`, declarations, immutable fragment ownership, and
  `CoreOp::External`;
- update builders, printer/dumper, verifier, editor checks, analyses, every closed
  optimizer match, and Core `None` goldens;
- retain `Unknown/Never/Opaque` and ordered operands; and
- add corruption, DCE/CSE/non-speculation, determinism, and scale tests.

### Stage 7B.2 — Compiler-owned intrinsic registry

- define the first small closed semantic keys, source signatures, and explicit
  canonical Stage 7 Core representations;
- connect Stage 7 context/cardinality/type checking to HIR external nodes;
- lower through structured Minecraft IR only;
- add exact target capability and unsupported-target failures; and
- prove source/HIR/Core/target origin continuity and server behavior.

### Stage 7B.3 — Static unsafe escape hatch

- add one source literal form and the explicit `unsafe minecraft(...)` statement;
- allocate immutable raw fragment descriptors in source order;
- split reusable target-independent fragment-shape checks from final selected-target
  raw-line validation without claiming Brigadier parsing;
- force the fixed conservative contracts at construction and verification; and
- test reload failures separately from compiler physical-line diagnostics.

### Stage 8 — Representations

- represent every new semantic operand/result without string erasure;
- make conversions and physical homes explicit;
- include bridge costs in external lowering recipes; and
- keep command success/result/absence distinct through the selected representation.

### Stage 9 — Scheduling consumption

- consume intrinsic context/fork/work contracts;
- preserve external operations as atomic continuation boundaries;
- reject proofs blocked by raw unknown behavior; and
- cover command-sequence/fork limits on the official server.

### Stage 10 — Runtime templates and macros

- add parsed typed templates and compiler-owned serializers;
- extend target IR with validated macro lines and typed macro calls;
- validate target capability, NBT argument frames, substitution failures, and cache
  cost;
- compare macro, static specialization, and dispatch recipes; and
- keep language macro expansion independent.

Every implementation tranche closes with `None` reference behavior, verifier
corruption tests, deterministic dumps/traces/packs, warnings-denied fast gates, and
the proportionate pinned Java 26.2 proof.

## Explicit non-goals

- user-defined external operations or user-asserted purity/effect contracts;
- parsing arbitrary Brigadier syntax into safe typed semantics;
- storing unchecked command text directly on a Core instruction;
- lowering from HIR straight to `MinecraftProgram`;
- treating `Bool` as command success or `Int32` as command result implicitly;
- runtime string concatenation as command construction;
- implementing Minecraft function macros in Stage 7;
- teaching Stage 9 to schedule through unknown raw behavior; and
- adding a generic dialect/interface/plug-in system around one closed registry.

## Primary references

- [MLIR rationale: side effects and speculation](https://mlir.llvm.org/docs/Rationale/SideEffectsAndSpeculation/)
- [MLIR operation interfaces](https://mlir.llvm.org/docs/Interfaces/)
- [MLIR symbols, references, and visibility](https://mlir.llvm.org/docs/SymbolsAndSymbolTables/)
- [LLVM Language Reference: inline assembler expressions](https://llvm.org/docs/LangRef.html#inline-assembler-expressions)
- [LLVM Language Reference: memory-effect attributes](https://llvm.org/docs/LangRef.html#memory-effects)
- [Rust Reference: inline assembly options and safety contracts](https://doc.rust-lang.org/reference/inline-assembly.html#options)
- [Minecraft snapshot 23w31a: function macros, substitution, and runtime cost](https://feedback.minecraft.net/hc/en-us/articles/18619031671821-Minecraft-Java-Edition-Snapshot-23w31a)
- [Minecraft Java 26.2 release notes and data-pack format 107.1](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
- [Stage 3 structured Minecraft IR plan](stage-3-minecraft-ir-plan.md)
- [Stage 0 foundational decisions](stage-0-decisions.md)
