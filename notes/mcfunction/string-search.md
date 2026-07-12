# How Do We Search Through a String?

## Short answer

Minecraft provides constant-index string slicing through the `string` source of
`data modify`, but no documented native `contains`, `find`, or dynamic-index string
operation. General runtime search therefore needs a compiler-generated algorithm or
a different representation.

Mojang documents this source form:

```text
string <entity|block|storage> [path] [start] [end]
```

where `start` is inclusive and `end` is exclusive.

## Case 1: haystack and needle are compile-time constants

Evaluate the search in the compiler. Runtime cost: zero commands.

## Case 2: needle is static and indexes are statically bounded

For a small maximum string size, generate fixed slices and comparisons. This creates
larger pack output but keeps all commands pre-parsed and removes loop state.

This strategy is especially plausible for protocol tokens, small identifiers, enum
names, and fixed-format strings. It is not reasonable for arbitrary user text.

## Case 3: general runtime search

The initial candidate is a loop over candidate start positions:

1. obtain haystack and needle lengths;
2. stop when `start + needle_length > haystack_length`;
3. place `start` and `end` in a macro argument compound;
4. use one macro line to slice the candidate;
5. compare candidate and needle;
6. return the current index on equality;
7. increment and repeat.

Dynamic slicing template:

```mcfunction
$data modify storage mdl:runtime find.candidate set string storage mdl:runtime find.haystack $(start) $(end)
```

Status: **Derived/Candidate**. It needs a complete 26.2 test before becoming a
stdlib implementation.

## Dynamic NBT equality without embedding the string in syntax

Minecraft does not document a direct `if nbt_path_a == nbt_path_b` condition. A
candidate equality primitive can exploit whether `data modify ... set from` reports
a change:

```mcfunction
# Copy the left side into disposable scratch.
data modify storage mdl:runtime compare set from storage mdl:runtime find.needle

# Replacing it with an equal candidate should report no change; replacing it with a
# different candidate should report a change.
execute store success score #changed mdl.tmp run data modify storage mdl:runtime compare set from storage mdl:runtime find.candidate
```

Then `#changed == 0` represents equality, assuming both typed source paths exist and
the documented/observed no-change result is stable. This avoids injecting an
arbitrary runtime string into SNBT command syntax.

Status: **Candidate** and high-priority test. Missing paths and command failure must
not be confused with equality.

## Better representations for search-heavy strings

Repeatedly slicing an opaque NBT string is probably the wrong representation for a
search-heavy workload. Alternatives include:

- retain an accompanying list of characters or tokens when the string is built;
- intern strings and compare/search integer IDs when the domain is finite;
- compile known needles into a trie or state machine;
- tokenize once and search tokens rather than characters;
- keep both raw and indexed representations when reuse repays construction cost.

The hard part is converting arbitrary raw input into characters efficiently. If MDL
created the string itself, it may be able to preserve pieces or tokens and avoid ever
flattening them before the search.

## Search is a representation-changing optimization problem

The source operation:

```mdl
haystack.find(needle)
```

should not lower immediately. The optimizer needs facts such as:

- are either values constant?
- is the maximum length known?
- is the needle from a finite set?
- will the same haystack be searched repeatedly?
- is byte, Unicode scalar, or UTF-16-style indexing promised by the language?
- is only equality with a prefix/suffix actually required?

`starts_with`, `ends_with`, `split_once`, and finite-token matching can be much
cheaper than general substring search and should remain distinct IR operations.

## Required tests

- exact `data get` string-length result on 26.2;
- negative and out-of-range slice indexes;
- empty needle and empty haystack;
- quote, backslash, newline, and Unicode behavior;
- the indexing unit used by slicing;
- dynamic equality through no-change detection;
- recursive versus unrolled loop cost;
- repeated-search break-even point for an indexed representation.

## Source

- [Mojang's `data modify ... string` specification](https://feedback.minecraft.net/hc/en-us/articles/13987663727757-Minecraft-Java-Edition-1-19-4)

