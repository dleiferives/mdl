# PS-N Validation Program Template

Copy this file when accepting a post-Brainfuck validation program. Assign a stable
PS identifier and decide explicitly whether it blocks Stage 9 work.

## Identity

- **Program:**
- **Substage:** PS-
- **Status:** proposed / planned / implementing / complete
- **Owner documents:** plan, todo, source package, cases
- **Required before Stage 9?:** yes/no, with reason

## Why this program

State what combination of language, compiler, Minecraft, and cost behavior this
program validates that existing capstones do not. “It is interesting” is not a
sufficient compiler exit criterion.

## User-visible contract

Freeze inputs, outputs, persistent state, failure behavior, limits, determinism, and
Minecraft target requirements. Separate language semantics from one selected
physical representation.

## Capability-gap audit

| Required capability | Exists? | Owner layer | Evidence | Work needed |
| --- | --- | --- | --- | --- |
| | | language / stdlib / intrinsic / runtime | | |

For every missing capability, decide whether it is:

- a general PS-2-style compiler/library slice;
- application code;
- a Stage 9 suspension/scheduling requirement;
- a later optimization opportunity; or
- deliberately unsupported.

Do not add an application-specific intrinsic merely to close the table.

## Program architecture

List independently testable modules and boundaries. Identify which parts execute in
the Core evaluator, which require vanilla, and which require a connected client or
other external fixture.

## Case corpus

Include minimal positive, boundary, negative, scale, and composition cases before a
showcase case. State the oracle for each case.

## Cost and scheduling pressure

Record static inputs to command/fork/storage cost, expected cardinalities, runtime
size distributions, and whether work can fit synchronously. If correctness requires
suspension, stop and feed the concrete requirement into Stage 9 rather than hiding a
scheduler in application/library code.

## Exit criteria

- [ ] Program uses only public language/standard/platform APIs.
- [ ] Every discovered general gap has its own design and tests.
- [ ] Core/server/client oracle assignments are explicit.
- [ ] All selected optimization policies are semantically equivalent.
- [ ] Target prerequisites and limits are reported honestly.
- [ ] Normal and abnormal cleanup contracts are tested.
- [ ] Footprint/cost/recipe selection is inspectable.
- [ ] The roadmap states whether this milestone blocks scheduling work.

