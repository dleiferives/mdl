# Stage 0: Foundational Compiler Decisions

Status: **Accepted for the first vertical slice**

Target baseline: **Minecraft Java 26.2, data-pack format 107.1**

These decisions are the design boundary for the first working compiler. They are
specific enough to begin implementation without pretending that syntax, data
representation, or optimization research is finished.

## 1. The compiler is implemented in Rust

Rust is the host implementation language.

Reasons:

- it can produce a normal native compiler without requiring users to build LLVM;
- algebraic data types and exhaustive matching fit typed IRs and rewrite passes;
- explicit ownership is useful for separating immutable IR snapshots from mutable
  pass state;
- its tooling, testing, serialization, and command-line ecosystems fit a compiler
  plus server-test harness;
- users can build from source with the Rust toolchain. Prebuilt binaries may be
  offered eventually, but maintaining them is not required by the architecture.

This rejects V and OCaml as implementation languages for the initial compiler. It
does not reject borrowing compiler-design ideas or algorithms from OCaml systems.

## 2. The initial target is vanilla Minecraft Java 26.2

The first backend targets exactly:

```text
edition: Java
version: 26.2
data-pack format: 107.1
environment: vanilla dedicated server
```

The compiler does not initially require a client, mod loader, command mod, plugin,
or custom server. Client-driven visual and interaction tests are a later testing
layer, not a requirement for ordinary programs.

Version-dependent behavior must live behind a target specification rather than be
scattered through optimization passes. Supporting another Minecraft version means
adding or updating a target profile and its conformance tests; `latest` is never an
implicit target.

## 3. We use a custom multilevel IR, not LLVM or MLIR

The compiler owns its IR and backend. LLVM and MLIR are neither build dependencies
nor user requirements.

The initial levels are:

```text
typed source/HIR
        -> typed SSA control-flow IR
        -> structured Minecraft IR
        -> laid-out command/function graph
        -> data-pack resources and .mcfunction text
```

We follow useful MLIR principles—verified operations, explicit types and effects,
progressive lowering, dialect-like separation, canonical forms, and readable IR
dumps—without adopting the MLIR implementation.

Not every source operation receives a one-to-one Minecraft operation, and not every
SSA basic block becomes an mcfunction. Physical layout is selected after semantic
optimization.

## 4. The language is strongly typed, including Minecraft context

The type system must understand Minecraft concepts rather than exposing everything
as strings:

```text
Player
Entity<T>
Selector<T, Cardinality>
Location
Dimension
Text
String
Score/Int
Nbt<T>
```

Function signatures may express execution-context requirements and effects. For
example, a `say` method can require an active player executor rather than accepting
an untyped selector string.

Cardinality, command result, command success, executor context, and ordinary
Booleans remain distinct until an explicit conversion or proven lowering connects
them.

The exact surface syntax and the complete type catalog are deferred. Their semantic
requirements are not.

## 5. The scheduler is static in the first implementation

The compiler can partition known work into statically generated phases and resume
those phases across ticks. Initial scheduling does not include:

- a dynamically growing job queue;
- runtime work stealing;
- arbitrary creation of new task implementations;
- a general dynamic allocator for continuations.

A task may store a generated program counter and live state, but all continuation
targets are known at compile time.

Compilation accepts target limits and policy budgets, including at least:

```text
max_command_sequence_length
max_command_forks
soft_commands_per_tick
```

The two gamerule limits are hard compatibility constraints. The per-tick budget is
a compiler policy used to partition work; it is not claimed to be an exact runtime
timer. Unbounded selector cardinality or loop work requires a user-provided bound,
a runtime guard, or a diagnostic.

## 6. There is no separately installed runtime

A compiled program is an ordinary datapack that runs on the target vanilla server.
Installing the compiler is sufficient to build it; running it requires only the
target Minecraft server.

The compiler may embed an internal datapack support library containing generated or
shared mcfunctions, objectives, storage, tags, and scheduler phases. That is part of
the output, not a separately installed runtime dependency.

The backend should erase unused support features. A program that needs no scheduler,
heap convention, or helper function should not pay for one.

## 7. Raw Minecraft commands are an explicit unsafe escape hatch

The language will permit raw commands so new Minecraft features and unusual
interoperability cases are not blocked on compiler releases.

The safe form uses typed interpolation where possible. Values inserted into command
syntax must use type-directed escaping and rendering; ordinary string concatenation
is not trusted command construction.

An opaque raw command is conservatively treated as:

```text
may read and write externally visible Minecraft state
may succeed or fail
may depend on and change execution context
may fork when its cardinality is unknown
blocks unsafe motion, duplication, and condition re-evaluation
```

More precise effect annotations may be added later, but false annotations are an
unsafe promise made by the programmer. Generated `.mcfunction` output and lowering
traces remain inspectable so users can understand the escape hatch in context.

## 8. Language macros and Minecraft macros are separate mechanisms

Language macros run during compilation and produce typed syntax or IR. Minecraft
function macros are a target primitive used when runtime values must enter command
syntax.

Using a language macro does not imply emitting a Minecraft macro, and the optimizer
may replace a Minecraft macro with specialization, static dispatch, or ordinary
commands.

## 9. Optimization is multi-objective and semantics-first

The required priority order is:

1. preserve typed program semantics;
2. respect target command-sequence, fork, and scheduling bounds;
3. minimize runtime work and worst-case tick cost;
4. reduce generated pack size and reload cost;
5. retain useful correspondence between source, IR dumps, and emitted functions.

Line count is recorded but is not treated as a faithful performance metric. The cost
model must distinguish command contexts, execute stages, function invocations,
selector forks, macro parsing/cache behavior, score operations, NBT operations, and
generated size.

Optimization policy should eventually support different goals such as `speed`,
`size`, and `balanced`, while producing identical observable semantics.

## 10. Correctness is tested at multiple levels

The compiler requires:

- unit tests for types, verification, analyses, and individual lowerings;
- snapshot tests for stable, readable IR and generated datapacks;
- differential tests that compile one program through alternative lowerings;
- integration tests on the pinned vanilla server;
- explicit tests near command-sequence and fork limits;
- deterministic compiler output for identical inputs and target options.

The real server is the semantic authority for emitted commands. A mock interpreter
can accelerate tests, but it cannot replace vanilla integration tests.

## 11. The first vertical slice intentionally excludes several systems

The following are deferred rather than implicitly decided:

- final source syntax and language name;
- a stable public package/module ecosystem;
- the full standard library;
- dynamic scheduling and arbitrary runtime job creation;
- e-graphs/equality saturation;
- automatic profile-guided optimization;
- every possible Minecraft version;
- client-only behavior and automated visual testing;
- a stable ABI between separately compiled datapacks.

The first slice constructs typed IR directly in Rust, lowers integer/Boolean control
flow to a datapack, runs it on the pinned server, and checks the result.

## Decision change policy

These decisions can change when implementation or measured server behavior provides
contrary evidence. Changes should be recorded explicitly with:

- the decision being replaced;
- the concrete problem or measurement;
- migration impact on the IR, emitted datapacks, and tests.

Ordinary experimentation with alternative lowerings does not require changing this
record.
