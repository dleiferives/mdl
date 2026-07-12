# How Do We Concatenate Strings?

## Best answer by situation

### Both strings are compile-time constants

Concatenate them in the compiler. Runtime cost: zero commands.

```mdl
"hello " + "world"
```

should emit the single literal `"hello world"` wherever it is consumed.

### The consumer accepts structured text

Do not concatenate strings. Preserve a `Text` tree and emit multiple components.
This is normally the right implementation for chat, titles, names, and other text
component consumers.

Conceptually:

```mdl
Text::concat([Text::literal("Kills: "), Text::score(player, kills)])
```

The backend emits one structured text component rather than constructing a new NBT
string. Runtime string allocation and macro parsing are avoided.

### A real runtime NBT string is required

Use a minimal macro function:

```mcfunction
# data/mdl/function/internal/string/concat.mcfunction
$data modify storage mdl:runtime concat.out set value "$(left)$(right)"
```

With arguments already in one compound:

```snbt
{left:"hello ",right:"world",out:""}
```

invoke:

```mcfunction
function mdl:internal/string/concat with storage mdl:runtime concat
```

Status: the macro substitution mechanism is **Documented**. The example is a
**Candidate** until tested on 26.2 with escaping-heavy strings.

## Concatenate more than two pieces at once

If all pieces are already macro arguments, one template can concatenate all of them:

```mcfunction
$data modify storage mdl:runtime concat.out set value "$(a)$(b)$(c)$(d)"
```

The compiler should flatten nested source concatenations before lowering. It should
not generate three runtime concatenations for `(a + b) + (c + d)` when one macro
instantiation can materialize the final string.

## Destination-sensitive lowering

String concatenation should remain a high-level operation until its consumer is
known:

```text
Concat(String pieces)
```

can become:

- constant folding;
- a composite `Text` value;
- one macro-created NBT string;
- a command template with the pieces inserted directly;
- no operation at all after inlining into a surrounding macro.

Materializing an intermediate string too early prevents all of these optimizations.

## Escaping is part of the type system

`"$(left)$(right)"` is only correct if Minecraft's macro encoding and the surrounding
SNBT quotes safely represent every allowed source string. The compiler needs a
version-specific encoder and adversarial tests. Until those pass, this operation
must not claim to support arbitrary strings.

Potential internal distinctions include:

```text
PlainString
SnbtStringContent
CommandToken
ResourceId
Text
```

They must not be freely interchangeable.

## Optimization rules

1. Fold constant pieces.
2. Flatten nested concatenations.
3. Drop empty pieces.
4. Fuse concatenation into the consuming macro when safe.
5. Preserve structured text instead of flattening it.
6. Reuse a materialized result only if reuse pays for storage and invalidation.

## Required benchmarks

- one macro concatenation versus multiple materializations;
- cache hit versus cache miss for repeated strings;
- direct composite text versus prebuilt NBT string;
- short versus large strings;
- quoting and Unicode correctness.

## Source

- [Mojang's function macro specification](https://www.minecraft.net/en-us/article/minecraft-snapshot-23w31a)

