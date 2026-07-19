# PS-3: Brainfuck Capstone

Status: **complete; see [`ps-3-handoff.md`](ps-3-handoff.md)**

## Objective

Write a complete implementation of the frozen Brainfuck contract as an ordinary MDL
package, compile it through the production pipeline, and execute representative
book-to-output cases on the pinned vanilla server.

PS-3 validates composition. It does not add a hidden Brainfuck Core operation,
compiler plugin, raw-command shortcut, or privileged access to private runtime
storage. A missing facility discovered here is reported as a PS-2 capability gap and
implemented/tested generically before the capstone resumes.

## Implemented package

```text
tests/programs/brainfuck/
  README.md
  cases.json
  book-cases.json
  src/
    main.mdl
    opcode.mdl
    byte.mdl
    parser.mdl
    tape.mdl
    io.mdl
    interpreter.mdl
```

Exact filesystem package conventions remain Stage 12 work. The test corpus loads
these files through the existing public in-memory package API while retaining their
logical module/source identities.

## Program architecture

Keep four independently testable boundaries:

```text
Book/holder adapter
  -> semantic program text
  -> parser and bracket validation
  -> normalized opcode program
  -> fuel-bounded interpreter
  -> byte output
  -> selected Minecraft output adapter
```

The parser, tape/zipper, and interpreter use public language/standard-library APIs.
The book adapter uses typed Minecraft platform operations. Tests can enter at each
boundary so a book failure does not impersonate an interpreter failure.

## Interpreter representation

The provisional design uses:

- a normalized opcode value;
- a two-list program cursor or another public cursor abstraction;
- a two-list zero-extending tape zipper;
- normalized `Int32` cells implementing the frozen byte semantics;
- an owned input byte list;
- an owned output byte builder/list;
- finite remaining fuel; and
- a closed completed/limit/error result.

These are application/standard-library values. Physical score/NBT layouts and
compiler frames remain invisible.

## Execution tiers

Develop and retain all tiers rather than keeping only the final large test:

1. **Interpreter tier:** pre-normalized opcodes and bytes, Core evaluator where
   supported, no Minecraft book dependency.
2. **Parser tier:** runtime semantic string to opcodes/result.
3. **Book-value tier:** controlled written-book value/holder to program text on a
   clientless server where the selected target operation permits it.
4. **Player-held tier:** explicitly deferred by the frozen PS-2.0 decision; the
   holder-independent exact-one armor-stand proof is the PS-3 gate.
5. **Full vertical tier:** held book through parse/execute/output.

The connected client should be the narrowest reliable external fixture available.
Do not implement a general Minecraft client, gameplay bot framework, or substitute
server implementation. Pin its version/protocol and isolate it from fast/default
tests.

## Output

Tests compare semantic byte output. A user-facing adapter may additionally render
bytes/text through a typed Minecraft operation. Do not make server log formatting
the only interpreter oracle.

If `say` is used, executor attribution is a separate expected effect. Byte-to-text
decoding and invalid-byte behavior require an explicit contract.

## Case corpus

Retain small, diagnostic cases before a showcase program:

- empty/comment-only;
- one increment/output;
- wrap upward/downward;
- move right/left and revisit cells;
- input echo and end-of-input;
- simple zero/nonzero loop;
- nested loops;
- ignored book prose and multiple pages;
- unmatched open/close;
- fuel zero, exact, one short, and surplus;
- maximum accepted program/input boundaries; and
- one recognizable program such as `A` or `Hello World` if it fits the frozen
  synchronous deployment bounds.

Each case states entry tier, program, input, fuel, expected result/output, and whether
vanilla execution is required. Avoid one enormous exact pack golden.

## Differential and deployment evidence

Compile the package under all four policy combinations with separate namespaces in
one server lifecycle where possible. Compare normalized semantic outputs, typed
errors, completion, public Minecraft effects, and private-runtime cleanup.

Record separately:

- Core and Minecraft optimization reports;
- exact emitted footprint;
- conservative command/fork/storage work;
- strict synchronous compatibility under declared program/fuel limits;
- selected list/string/book/macro recipes; and
- measured wall time/reload only under the measurement protocol.

Optimizations may change command files and physical representations but not the
capstone result.

## Failure and cleanup

The following are distinct:

- invalid program;
- no/wrong book;
- program/input bound exceeded;
- semantic fuel exhausted;
- strict compile/deployment limit rejection;
- abnormal Minecraft command-limit interruption; and
- connected-client/setup failure.

Normal typed errors and fuel exhaustion must leave compiler frames balanced.
Abnormal target interruption retains Stage 8's partial-state/recovery contract and
must not be reported as a normal Brainfuck result.

## Non-goals

- executing every Brainfuck program synchronously;
- optimizing specifically by recognizing the Brainfuck interpreter;
- benchmarking against native interpreters as a language-quality gate;
- scheduling across ticks;
- a stable external package ABI;
- a complete Minecraft client test platform; and
- accepting arbitrary book text as raw command syntax.

## Exit criteria

- The package uses only public MDL and bundled standard/platform library APIs.
- Parser, tape, interpreter, book adapter, and output adapter have independent tests.
- The frozen Brainfuck semantics pass the case corpus.
- All compiler policy products are semantically equivalent.
- At least one complete written-book-to-output path executes on pinned vanilla.
- If actual-player-held input is a required frozen gate, the pinned client fixture
  executes it; otherwise the deferral is explicit and holder-independent behavior is
  proven.
- Invalid input, fuel exhaustion, and deployment incompatibility are distinct.
- Normal completion/error paths leak no compiler-private activation state.
- Footprint, cost, selected recipes, and target prerequisites are inspectable.
- Any discovered general gap was repaired in PS-2 with its own evidence.

All required criteria are closed by the [PS-3 handoff](ps-3-handoff.md). Strict
deployment enforcement, static fuel-to-command proofs, rich book-component variants,
and connected-player automation remain the explicit accepted deferrals rather than
retroactive capstone requirements.
