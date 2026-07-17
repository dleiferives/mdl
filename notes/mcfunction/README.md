# mcfunction Backend Notebook

This directory answers one backend question per file: how a language operation can
actually be represented and executed by a Minecraft Java data pack.

The target is Minecraft Java 26.2, data-pack format 107.1. The commands discussed
here are not assumed to be equally fast merely because they occupy the same number
of lines. Minecraft counts execution stages, function invocations, and forked
contexts separately; a single `execute as @e` line can therefore be much more
expensive than several scalar scoreboard commands.

## Evidence labels

- **Documented**: described by Mojang release notes.
- **Derived**: follows from documented command behavior, but the composition is our
  own design.
- **Candidate**: plausible compiler strategy that needs a real-server test.
- **Measured**: observed on the target vanilla server with the experiment recorded.

Many executable recipes still need to become automated 26.2 integration tests.
OpenJDK 25.0.3 and the official vanilla 26.2 server are now available for doing so;
the command-limit, scheduling, native range, and float-conversion claims have
received direct server tests.

## Questions answered so far

- [Which native value carriers and command domains exist in Java 26.2?](native-value-carriers.md)
- [What is the complete scoreboard surface and its arithmetic behavior?](scoreboard-operations.md)
- [What exactly does scoreboard `*` select, and can it match patterns?](scoreboard-wildcard.md)
- [How do lists, compounds, dictionaries, and higher-order operations compose?](list-compound-and-higher-order.md)
- [What is the exact list-operation algebra, and what structures compose from it?](list-operation-algebra.md)
- [What do existing collection libraries and list-heavy datapacks teach us?](lists/README.md)
- [Which list experiments and source inspections have been recorded?](lists/research-ledger.md)
- [What do existing NBT/dictionary libraries and datapacks teach us?](nbt/README.md)
- [Which NBT/dictionary experiments and source inspections have been recorded?](nbt/research-ledger.md)
- [Which range domains exist, how do they compose, and what are native float semantics?](ranges/README.md)
- [Which range/float probes and public implementations have been recorded?](ranges/research-ledger.md)
- [How should macros be used as a backend primitive?](macro-composition.md)
- [How do we concatenate strings?](string-concatenation.md)
- [How do we search through a string?](string-search.md)
- [How do we append one element to a list?](list-append-one.md)
- [How do we append many elements to a list?](list-append-many.md)
- [How do we convert integers and floats?](numeric-conversion.md)
- [How do we increment a value?](increment.md)
- [Can one command update many values?](bulk-operations.md)
- [How should branches and basic blocks be lowered?](branches-and-control-flow.md)
- [How should typed, block-scoped execute chains work?](execute-chains.md)
- [How should execution context and coordinate frames interact?](coordinate-frames.md)
- [How do we access a dynamic field or index?](dynamic-access.md)
- [How do we implement loops?](loops.md)
- [How should the compiler select runtime representations?](representation-selection.md)
- [What are the command limits, and how can work span ticks?](command-limits-and-multi-tick.md)
- [Which primitive questions should we research next?](research-queue.md)

The reusable vanilla research fixture for the three new inventories lives under
[`fixtures/native-primitives-26.2`](fixtures/native-primitives-26.2/README.md). Run
its world on an isolated port; it is research input, not compiler runtime code.

Stage 8 turns the score/NBT and synchronous execution research into compiler
evidence. Scalar recursive SCCs use compiler-private command-storage tail fields
with `Bool` stored as byte and `Int32` as int; acyclic and serial selector children
reuse static scores. The measured behavior, abnormal-termination contract, and exact
compiler protocol are recorded in
[`../compiler/stage-8-completion-audit.md`](../compiler/stage-8-completion-audit.md).

## Initial backend rule

A source type does not imply one runtime representation. The compiler should track
a set of legal representations and select one using whole-region usage information.
For example:

- a hot mutable integer normally belongs in a scoreboard;
- an integer used only as command NBT can remain an NBT integer;
- a numeric value used as a float only at an output boundary can be converted during
  `execute store`;
- a newly constructed list should be materialized once, not appended element by
  element;
- a `Text` value should normally remain a structured text component rather than be
  flattened into a string;
- a bounded dynamic array can use specialization or dispatch, while a truly dynamic
  array may justify a macro-indexed NBT path.

This means representation selection, scalar replacement, batching, and loop
specialization are core compiler passes—not library afterthoughts.

## Cost dimensions to record

Every candidate implementation should eventually be measured along at least these
dimensions:

1. pre-parsed command executions;
2. `execute` stages;
3. contexts created by selector forks;
4. function invocations;
5. macro cache hits and misses;
6. scoreboard reads/writes;
7. command-storage/NBT reads, writes, copies, and list shifts;
8. temporary commands needed to bridge representations;
9. generated pack size and load/reload time;
10. peak commands and wall time per tick.

Mojang explicitly documents that macro lines are reparsed after substitution and
that repeated argument sets may be cached. Mojang also documents that command-chain
accounting includes individual command contexts, `execute` stages, and function
invocations. These facts are the starting point for the cost model, not a complete
performance model.

## Primary references

- [Minecraft Java 26.2 release](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
- [Function macros and their performance considerations](https://www.minecraft.net/en-us/article/minecraft-snapshot-23w31a)
- [Function execution, return, fork, and operation limits](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-3)
- [`data modify` list operations](https://www.minecraft.net/pl-pl/article/village---pillage-out-java-)
- [`data modify ... string` slicing](https://feedback.minecraft.net/hc/en-us/articles/13987663727757-Minecraft-Java-Edition-1-19-4)
