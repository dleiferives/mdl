# How Should Macros Be Used as a Backend Primitive?

## Short answer

Minecraft function macros are a typed dynamic-command-construction boundary. They
should be used when a runtime value must become part of command syntax: an NBT path
index, resource identifier, selector literal, numeric literal, or fragment of an
SNBT value. They should not be the default implementation of normal arithmetic,
branching, or function parameters.

Mojang documents that only lines beginning with `$` are substituted and reparsed.
Ordinary lines in the same function remain pre-parsed. A parameter compound must
contain every referenced variable, syntax failure skips the function call, and the
game attempts to cache instantiated commands for repeated parameter sets.

Status: **Documented**, with the current 26.2 cache size and exact replacement
escaping still requiring source inspection or server tests.

## Core compiler model

Treat a macro invocation as:

```text
instantiate(command_template, typed_argument_tuple) -> parsed_command
execute(parsed_command)
```

It is not:

```text
evaluate_arbitrary_source_expression
```

The type system should therefore distinguish values by the syntax positions into
which they can safely be encoded:

```text
MacroInt
MacroFloat
MacroSnbt<T>
MacroNbtKey
MacroNbtIndex
MacroResourceId<K>
MacroSelectorFragment
MacroCommandFragment       // unsafe/checked-native boundary only
```

A normal `String` must not automatically become a command fragment.

## Make the macro boundary as small as possible

If only one operation needs a dynamic index, generate one macro line and leave the
rest of the work in an ordinary function:

```mcfunction
# mdl:internal/read_index.mcfunction
$data modify storage mdl:runtime result set from storage mdl:heap values[$(index)]
```

Do not turn the caller's entire body into a macro function. That would reparse more
commands and enlarge the cache key surface for no semantic benefit.

## Preserve and exploit the cache

The optimizer should:

- include only actually referenced values in a macro argument tuple;
- keep stable argument ordering and encoding;
- specialize compile-time constants out of macro templates;
- separate a high-variance parameter from unrelated low-variance templates when
  doing so avoids repeated reparsing;
- hoist repeated macro calls when their arguments and observable result are stable;
- consider generating ordinary specialized functions for a small hot value set.

For example, a dynamic enum with four possible values may be faster as four
pre-parsed functions plus dispatch than as an unbounded macro. This must be measured,
not assumed.

## Macro ABI and temporary frames

Arguments can be supplied inline or from a compound in block, entity, or command
storage. Compiler-generated calls should use an explicit frame layout:

```snbt
{
  call_17: {
    index: 4,
    value: "hello"
  }
}
```

Static scratch frames are cheap but are unsafe for recursive or re-entrant code.
The compiler should perform frame-liveness analysis:

- reuse a static function-local frame when calls cannot overlap;
- allocate a stack/list frame when recursion or nested calls require preservation;
- avoid copying values already colocated in a suitable compound;
- scalarize a frame entirely when all arguments become compile-time constants.

Whether forked command contexts can safely share a scratch frame for a particular
lowering must be proven from actual vanilla execution order and tested.

## Important optimization choices

For any operation that could use a macro, compare at least:

1. ordinary commands plus a scoreboard/storage bridge;
2. one macro command with an existing storage compound;
3. value specialization into pre-parsed functions;
4. a bounded dispatch tree;
5. a different data representation that removes the dynamic syntax requirement.

The fifth choice will often win. The best dynamic index is sometimes an index the
compiler eliminated.

## Required tests

- string arguments containing both quote types, backslashes, newlines, and Unicode;
- every numeric NBT type and numeric suffix behavior;
- missing and extra fields;
- cache hit versus miss cost and actual cache capacity in 26.2;
- nested and recursive macro calls;
- calls across forked execution contexts;
- invalid generated resource IDs, paths, selectors, and SNBT;
- arguments with equal referenced fields but different ignored fields.

## Sources

- [Mojang's function macro specification and performance notes](https://www.minecraft.net/en-us/article/minecraft-snapshot-23w31a)
- [1.20.2 macro argument numeric formatting change](https://feedback.minecraft.net/hc/en-us/articles/19703470383757-Minecraft-Java-Edition-1-20-2)

