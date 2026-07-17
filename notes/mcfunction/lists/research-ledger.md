# Research and Experiment Ledger

This is the coverage record for the list survey. Every completed source inspection
or server probe gets an entry, even when the result is negative or already appears
in a thematic note. “Measured” means the target vanilla 26.2 server produced the
observation. “Source-derived” means the behavior was read from an implementation
and still needs an independent fixture if MDL will rely on exact semantics or cost.

## Target-server list probes

All rows below are implemented in
[`test/list_algebra.mcfunction`](../fixtures/native-primitives-26.2/datapack/data/mdl_native/function/test/list_algebra.mcfunction)
and were run successfully on the isolated `127.0.0.1:25585` vanilla server. The
server was stopped after the observations were captured.

| ID | Status | Probe | Observation | Written analysis |
| --- | --- | --- | --- | --- |
| L01 | Measured | Batch append from `[7,8]` | `[1,2,3] -> [1,2,3,7,8]`; source order retained | [`../list-operation-algebra.md`](../list-operation-algebra.md) |
| L02 | Measured | Batch prepend | Result `[7,8,1,2,3]`; batch order retained | same |
| L03 | Measured | Batch insert at 1 | Result `[1,7,8,2,3]` | same |
| L04 | Measured | Negative insert | `-1` appends; `-2` inserts before last | same |
| L05 | Measured | Insert bounds | `size` is valid; lowest negative bound is `-(size+1)` | same |
| L06 | Measured | Empty source selection | Append/prepend/insert make no change and return zero | same |
| L07 | Measured | Multi-source `set from xs[]` | Selects the last source only | same |
| L08 | Measured | Wildcard set on non-empty list | Broadcasts one value to every element | same |
| L09 | Measured | Wildcard set on empty list | Creates a singleton instead of performing zero iterations | same |
| L10 | Measured | `remove empty[]` | Returns zero and preserves the empty list field | same |
| L11 | Measured | Deep copy | Later nested source mutation does not affect copy | same |
| L12 | Measured | Self append/prepend/insert | Source selection is snapshotted before target mutation | same |
| L13 | Measured | Root list pattern | Pattern is unordered containment, not sequence equality | same; [`equality-membership-and-count.md`](equality-membership-and-count.md) |
| L14 | Measured | Duplicate expected pattern value | One actual element can satisfy repeated expected elements | same |
| L15 | Measured | Empty expected list | Matches only an empty actual list | same |
| L16 | Measured | Compound filtered path update | All matching records are updated in one command | same |
| L17 | Measured | Compound filtered path removal | All matching records are removed; result is match count | same |
| L18 | Measured | Wrong-type array append | Homogeneous NBT array rejects incompatible value | same |
| L19 | Measured | Tail stack push/pop | Append + copy/remove `[-1]` gives LIFO behavior | [`stacks-queues-and-zippers.md`](stacks-queues-and-zippers.md) |
| L20 | Measured | Naive FIFO front pop | Correct result but shifts remainder structurally | same |
| L21 | Measured | Two-stack FIFO | Pops `1,2` across intervening enqueue and retains logical `3,4` | same |
| L22 | Measured | Tail-worklist reverse | Heterogeneous `[1,"two",{n:3},[4]]` reverses correctly | [`traversal-direction-and-cost.md`](traversal-direction-and-cost.md) |
| L23 | Measured | No-op overwrite equality | Exact nested equality returns zero; difference returns one and overwrites scratch | [`equality-membership-and-count.md`](equality-membership-and-count.md) |
| L24 | Measured | Numeric NBT type equality | Overwriting int `1` with byte `1b` counts as one change | same |
| L25 | Measured | Bulk exact count | `length - broadcast changed-count` gives three matches in `[1,2,1,3,1]` | same |
| L26 | Measured | Unguarded empty exact count | Wildcard set mutates `[]` to `[needle]`; raw `length - changed` produces bogus `-1` | same |

## Community source inspections

| ID | Evidence | Source area inspected | Finding | Written analysis |
| --- | --- | --- | --- | --- |
| S01 | Source-derived | Bookshelf module inventory | Current 26.2 module covers 40+ transforms, predicates, folds/scans, generators, windows, and set operations | [`README.md`](README.md) |
| S02 | Source-derived | Bookshelf callback trampoline | Macro callback ID; value/index/accumulator input; result storage; predicate command-success channel | [`callbacks-and-frames.md`](callbacks-and-frames.md) |
| S03 | Source-derived | Bookshelf operation stack | Per-invocation copied frame makes nested operations re-entrant | same |
| S04 | Source-derived | Bookshelf map/filter/for-each | Stable left order, but repeated head removal is structurally quadratic | [`traversal-direction-and-cost.md`](traversal-direction-and-cost.md) |
| S05 | Source-derived | Bookshelf reverse/right folds/find-last | Tail consumption provides the linear physical primitive | same; [`folds-scans-and-search.md`](folds-scans-and-search.md) |
| S06 | Source-derived | Bookshelf any/all/none/find | `return` short-circuits callback execution | [`folds-scans-and-search.md`](folds-scans-and-search.md) |
| S07 | Source-derived | Bookshelf reduce/fold/scans | Explicit versus implicit seed and emitted intermediate accumulator conventions | same |
| S08 | Source-derived | Bookshelf contains/distinct | Attempted no-op overwrite is exact arbitrary-NBT equality | [`equality-membership-and-count.md`](equality-membership-and-count.md) |
| S09 | Source-derived | Bookshelf flat-map | Append callback result elements directly; fuse map+flatten | [`collection-shaping.md`](collection-shaping.md) |
| S10 | Source-derived | Bookshelf flatten-deep | Mutation success is used as a structural list probe; front worklist is costly | same |
| S11 | Source-derived | Bookshelf partition | One predicate call routes element into one of two outputs | same |
| S12 | Source-derived | Bookshelf zip | Pairs in order and truncates to the shorter input | same |
| S13 | Source-derived | Bookshelf slice | Normalizes negative bounds; inspected validation includes a suspicious contradictory condition, so do not copy semantics without tests | same |
| S14 | Source-derived | Bookshelf chunk/sliding | Clear reference algorithms; sliding recopies remaining input per window | same |
| S15 | Source-derived | Bookshelf range/repeat/generate | Tail-appending producers; signed-step/termination and zero-step need explicit semantics | same |
| S16 | Source-derived | Bookshelf distinct/set operations | Stable first-seen order with quadratic arbitrary-NBT fallback | [`set-like-operations.md`](set-like-operations.md) |
| S17 | Source-derived | stdmodulesystem indexed for-each | Score-to-macro index traversal avoids list shifts | [`traversal-direction-and-cost.md`](traversal-direction-and-cost.md) |
| S18 | Source-derived | stdmodulesystem remove-on-match | Descending index deletion avoids skipped elements and unnecessary shifts | same |
| S19 | Source-derived | stdmodulesystem exact count | Copy + broadcast set changed-count computes exact multiplicity | [`equality-membership-and-count.md`](equality-membership-and-count.md) |
| S20 | Source-derived | stdmodulesystem “like” operations | Macro SNBT compound filters perform bulk pattern get/update/remove | same |
| S21 | Source-derived | stdmodulesystem reference variants | Command-path strings emulate mutable references and avoid container round trips | [`references-and-ordered-maps.md`](references-and-ordered-maps.md) |
| S22 | Source-derived | stdmodulesystem plain map | Quoted macro compound key provides direct string-key lookup | same |
| S23 | Source-derived | stdmodulesystem iterable map | Dynamic-key compound entries carry prev/next links; inspected iteration follows reverse insertion order | same |
| S24 | Source-derived | stdmodulesystem multimaps | Map values compose existing list/set operations; child references enable in-place work | same |
| S25 | Source-derived | Arcensoth iteration | Pop-tail/append traversal is linear but reverses encounter/result order | [`traversal-direction-and-cost.md`](traversal-direction-and-cost.md) |
| S26 | Source-derived | intsuc Brainfuck | Two tail stacks implement bidirectional zipper cursor movement | [`stacks-queues-and-zippers.md`](stacks-queues-and-zippers.md) |
| S27 | Source-derived | intsuc growable queue | Ring region plus overflow batches capacity growth and avoids front deletion | same |
| S28 | Source-derived | intsuc bucket sort | Bounded numeric key becomes direct bucket position; sentinel slots removed afterward | [`specialized-indexes.md`](specialized-indexes.md) |
| S29 | Source-derived | intsuc list-mapped trie | Generated fixed 32-level nested-list path indexes signed scoreboard keys | same |
| S30 | Source-derived | intsuc nested-list indexing note | Copying subtrees can destroy nominal tree-complexity benefits | same |
| S31 | Negative result | DPlib repository | Surveyed current tree contains math, datetime, threading, and benchmarking modules but no reusable collection/list library | this row |

## Explicitly untested or unresolved

These are documented now so they are not mistaken for completed research:

| ID | Question | Next experiment |
| --- | --- | --- |
| U01 | Safe empty-list form of bulk exact-count idiom | Confirm unguarded failure, then specify/fixture the guarded zero-return wrapper |
| U02 | Relative 26.2 cost of head-pop, tail-pop, and dynamic-index traversal | Benchmark equal payloads and lengths, recording commands and wall time |
| U03 | Bookshelf slice edge semantics | Test negative bounds, clamping, equal/reversed bounds, and out-of-range values |
| U04 | Window edge semantics | Test partial final windows, zero/negative sizes/steps, and `step > size` |
| U05 | Deep-flatten type probe | Test ordinary lists, all array types, compounds, strings, and empty lists |
| U06 | Ring queue versus two-stack queue | Implement equivalent fixture APIs and measure steady-state plus refill latency |
| U07 | Ordered-map corruption/invalidation | Prototype typed links and test insert/update/remove/head/tail/singleton cases |
| U08 | List trie cost on current server | Compare against compound map and linear record scan at realistic sizes |
