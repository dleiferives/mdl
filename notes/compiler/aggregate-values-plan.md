# Aggregate Values — Post-Stage-8 Client Plan

Status: **planned; first fixed-layout client assigned to Stage 8.5 PS-2**

This is the next value-representation client of Stage 8. The Stage 8.5 roadmap now
assigns its first immutable fixed-layout slice to PS-2 because the Brainfuck
interpreter/parser needs structured state before persistent scheduling. The focused
ownership and exit plan is
[`pre-scheduler/ps-2/aggregate-values.md`](pre-scheduler/ps-2/aggregate-values.md).
Later aggregate/reference/layout capabilities remain follow-on work and must preserve
the Stage 8 physical boundary.

## First semantic slice

Start with immutable, fixed-layout structs containing only `Bool` and `Int32`.
Before lowering, freeze:

1. nominal versus structural type identity;
2. field declaration and deterministic layout order;
3. whole-value assignment and call/return copy semantics;
4. field projection and construction evaluation order;
5. whether equality exists and how it observes fields;
6. source `var` as an SSA-updated place rather than a shared reference; and
7. the absence of aliases, escaping field references, and interior mutation.

Lists, variable-size values, `mut` parameters, references, and Minecraft-backed
views are separate later slices.

## IR work

- Add a canonical aggregate type and typed construct/project operations to semantic
  Core only after source semantics are fixed.
- Keep aggregate values immutable in SSA; do not place NBT paths in Core.
- Extend verification, printing, editing, optimization effects, demand, and liveness
  with complete per-result contracts.
- Define observation barriers so scalar replacement never changes copy or mutation
  behavior.

## Physical candidates

For each exact use, let physical planning choose among a deliberately small set:

- fully scalarized field realizations in score storage;
- one typed NBT compound realization;
- mixed scalar fields plus a materialized compound at an observation boundary; and
- an elided aggregate whose demanded fields are compile-time facts.

Materializations must be explicit occurrences: field scores→compound, compound
field→score, compound copy, and direct construction into a destination. A semantic
aggregate may own several simultaneous equivalent realizations; mutable storage is
never itself the semantic value.

## ABI questions to answer before code

- Which aggregate sizes use exploded direct fields versus one indirect frame/NBT
  field?
- Is the internal ABI policy fixed, cost-selected, or caller/callee specialized?
- When is a wrapper required between public and internal ABI shapes?
- How are recursive aggregate parameters/results copied without aliasing an active
  caller frame?
- Which copies are semantic and which may be eliminated by verified destination
  construction?

This is where Stage 8's deferred frame-field parameter/result and typed frame-source
parallel-copy work becomes real rather than speculative.

## Initial vertical gate

Compile one small struct through construction, field access, ordinary calls,
branches, return, a serial run-body-local use, and terminating recursion. Compare
scalarized and compound-compatible policies structurally and on the pinned server.
Require exact realization/materialization reports, no unplanned NBT bridge, no alias
escape, deterministic artifacts, corruption tests, and absence of aggregate runtime
support when unused.
