# Which Primitive Questions Should We Research Next?

Each question below should eventually get its own file containing semantics,
candidate implementations, optimization rules, failure behavior, and 26.2 tests.

## Values and comparison

- integer, fixed-point, NBT-number, string, and compound equality;
- ordering and range comparison;
- Boolean representation and negation;
- `Option<T>` and `Result<T, E>` representation;
- command success versus command result;
- null/missing NBT paths;
- enum and tagged-union representation.

## Integer and real arithmetic

- add, subtract, multiply, divide, remainder, min, max, and swap;
- constants without permanent scoreboard slots;
- overflow detection and checked/saturating/wrapping modes;
- fixed-point rescaling;
- rounding, floor, ceiling, and absolute value;
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

1. dynamic NBT equality;
2. list get/set/remove by dynamic index;
3. integer arithmetic and overflow;
4. function arguments, return values, and frame layouts;
5. if/match and finite-domain dispatch;
6. string length/slice and exact Unicode semantics;
7. selector/query planning;
8. benchmarking methodology on the vanilla 26.2 server.

