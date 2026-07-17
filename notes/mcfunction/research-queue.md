# Which Primitive Questions Should We Research Next?

Each question below should eventually get its own file containing semantics,
candidate implementations, optimization rules, failure behavior, and 26.2 tests.

## Answered since this queue was opened

- Native carriers and command domains: [`native-value-carriers.md`](native-value-carriers.md)
- Scoreboard arithmetic, wildcard behavior, and regex boundary:
  [`scoreboard-operations.md`](scoreboard-operations.md) and
  [`scoreboard-wildcard.md`](scoreboard-wildcard.md)
- Basal list operations, dynamic indices, exact NBT equality/count, matching,
  stack/queue/reverse, and source-derived higher-order algorithms:
  [`list-operation-algebra.md`](list-operation-algebra.md) and
  [`lists/README.md`](lists/README.md)
- Macro arguments and dynamic path safety: [`macro-composition.md`](macro-composition.md)
- NBT compounds, dynamic keys, dictionary representations, reflection, references,
  snapshots, schemas, and identity: [`nbt/README.md`](nbt/README.md)
- Linear/cyclic ranges, runtime interval algebra, progressions, native float
  conversion, and public float representations: [`ranges/README.md`](ranges/README.md)

The exhaustive list research/test coverage ledger is
[`lists/research-ledger.md`](lists/research-ledger.md). Consult it before repeating
an experiment.

The equivalent NBT/dictionary ledger is
[`nbt/research-ledger.md`](nbt/research-ledger.md).

The equivalent range/floating-point ledger is
[`ranges/research-ledger.md`](ranges/research-ledger.md).

## Values and comparison

- integer, fixed-point, NBT-number, string, and compound equality;
- arbitrary native-float ordering remains open; integer, stopwatch, selector, and
  predicate range domains are catalogued;
- Boolean representation and negation;
- `Option<T>` and `Result<T, E>` representation;
- command success versus command result;
- null/missing NBT paths;
- enum and tagged-union representation.

## Integer and real arithmetic

- add, subtract, multiply, divide, remainder, min, max, and swap;
- constants without permanent scoreboard slots;
- overflow detection and checked/saturating/wrapping modes;
- fixed-point rescaling and overflow-safe multiply/divide;
- a total rounding contract across fixed, decimal, native float, and command-result
  boundaries;
- powers, roots, trigonometry, logarithms, and lookup-table generation;
- random numbers and deterministic sequences.

## Strings and text

- length, slice, character/code-unit access, and iteration;
- equality without macro injection;
- prefix/suffix tests;
- split, replace, trim, case conversion, and formatting;
- string-to-number and number-to-string conversion;
- interning and finite string domains;
- structured `Text` construction, styling, translation, and NBT-backed pieces;
- safe macro escaping for every syntax position.

## Lists, tuples, and maps

- list allocation, copy, get, set, insert, remove, clear, and length;
- append-range and list concatenation;
- contains/find/index-of;
- map, filter, fold/reduce, sort, reverse, and deduplication;
- queue, stack, ring buffer, and priority queue;
- fixed-size tuple/array scalarization;
- dynamic key lookup in compounds;
- hash table, sorted table, trie, and generated dispatch alternatives;
- array-of-structs versus struct-of-arrays transformations.

## Functions and control flow

- calls with no arguments, static arguments, runtime arguments, and macro arguments;
- return values, failure, early return, and multiple returns;
- recursion, tail recursion, and stack-frame layouts;
- if/else, match, short-circuit Boolean logic, and switch dispatch;
- static, bounded, dynamic, entity, and tick-split loops;
- closures/captures and execution-context capture;
- inlining, outlining, specialization, and shared suffix/prefix extraction.

## Minecraft-specific values

- exact-one, optional, and many-entity references;
- online player identity versus persistent player ID;
- positions, rotations, anchors, dimensions, and local coordinates;
- blocks, block states, items, components, inventories, and slots;
- resources and tags with registry-typed IDs;
- selectors as typed query plans;
- command context (`as`, `at`, `in`, `positioned`, `rotated`, `anchored`);
- storage, scoreboards, bossbars, teams, tags, and world clocks;
- predicates, loot tables, item modifiers, advancements, and function tags.

## Runtime and whole-program behavior

- global/static initialization and `minecraft:load`;
- per-tick execution and event/advancement-driven dispatch;
- scheduling and cancellation;
- coroutine/state-machine lowering across ticks;
- allocation, scratch-space reuse, stack frames, and cleanup;
- persistence and schema migration across pack versions;
- re-entrancy and forked-context safety;
- atomicity and observable intermediate state;
- interoperability with raw commands and other data packs.

## Optimizer-specific experiments

- the true cost of one function call;
- the true cost of each `execute` stage;
- selector scan and fork costs by cardinality and selector shape;
- scoreboard versus command-storage throughput;
- NBT path cost by depth, wildcard use, and list size;
- NBT copy cost by value size;
- macro cache capacity, hit/miss cost, and argument locality in 26.2;
- generated dispatch versus macro access thresholds;
- unrolling versus pack size/reload time;
- batching break-even points;
- profile collection through `tick`, `perf`, JFR, Tracy, or a controlled harness.

## Suggested next batch

The next highest-leverage answers are:

1. representation-level benchmarks for tail, indexed, zipper, and ring list forms;
2. fixed-point scale selection, overflow-safe multiplication/division, and rounding;
3. typed function arguments, return values, effects, and frame layouts;
4. if/match and finite-domain dispatch;
5. exact Unicode semantics for string length/slice/iteration;
6. selector/query planning;
7. dictionary/index benchmarks for compound maps, record scans, buckets, and tries;
8. benchmarking methodology on the vanilla 26.2 server;
9. predicate interval edge cases and arbitrary float comparison policy.
