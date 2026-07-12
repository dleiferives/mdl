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

Most executable recipes still need to become automated 26.2 integration tests.
OpenJDK 25.0.3 and the official vanilla 26.2 server are now available for doing so;
the command-limit and scheduling claims have received their first server tests.

## Questions answered so far

- [How should macros be used as a backend primitive?](macro-composition.md)
- [How do we concatenate strings?](string-concatenation.md)
- [How do we search through a string?](string-search.md)
- [How do we append one element to a list?](list-append-one.md)
- [How do we append many elements to a list?](list-append-many.md)
- [How do we convert integers and floats?](numeric-conversion.md)
- [How do we increment a value?](increment.md)
- [Can one command update many values?](bulk-operations.md)
- [How should branches and basic blocks be lowered?](branches-and-control-flow.md)
- [How do we access a dynamic field or index?](dynamic-access.md)
- [How do we implement loops?](loops.md)
- [How should the compiler select runtime representations?](representation-selection.md)
- [What are the command limits, and how can work span ticks?](command-limits-and-multi-tick.md)
- [Which primitive questions should we research next?](research-queue.md)

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
