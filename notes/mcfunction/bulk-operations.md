# Can One Command Update Many Values?

## Short answer

Yes. Minecraft has true native bulk operations in addition to `execute` context
fan-out. This distinction should be explicit in the IR because the hard limits and
likely performance characteristics are very different.

The most important initial families are:

- scoreboard commands with a selector or `*` target;
- `data modify`/`data remove` with multi-match NBT paths;
- bulk source and target paths in one `data modify`;
- world commands such as `fill` and `clone`;
- commands whose own target argument accepts multiple entities.

## Increment multiple entity scores

This command incremented three armor-stand scores:

```mcfunction
scoreboard players add @e[type=minecraft:armor_stand,tag=mdl_bulk_test] mdl_test 1
```

The test function contained that command followed by a completion write. With
`max_command_sequence_length=3`, all three values became 1 and the completion write
ran. The three counted operations were the function invocation, the bulk scoreboard
command, and the storage write.

It also succeeded with `max_command_forks=1`. Therefore the scoreboard command's
native multi-target handling did not create command execution contexts in the way
`execute as` does.

Status: **Measured** on vanilla 26.2.

## Increment multiple fake players

The special score-holder target `*` updates every holder that already has a score in
the objective:

```mcfunction
scoreboard players add * mdl.vector 1
```

Three initialized fake players all moved from 0 to 1 in one command. The command
reported updating all existing holders in the objective, including unrelated holders.

This suggests a compiler layout optimization: place values that frequently receive
the same operation in a dedicated objective, then operate on the whole objective.
The objective becomes a physical vector.

Important constraints:

- `*` affects every existing score holder in that objective;
- absent scores are not created by the wildcard operation;
- there is no observed prefix/pattern wildcard for choosing three arbitrary fake
  player names;
- unrelated values must not share the vectorized objective.

## Apply one runtime scalar to every score

`scoreboard players operation` can use all existing holders as targets and one score
as the shared source operand:

```mcfunction
scoreboard players operation * mdl.vector += #delta mdl.source
```

In the 26.2 test, a source value of 2 was added to all holders. This generalizes
bulk updates beyond literal `add` and `remove`.

Candidate compiler operations include:

```text
vector += scalar
vector -= scalar
vector *= scalar
vector /= scalar
vector %= scalar
vector = min(vector, scalar)
vector = max(vector, scalar)
```

Each exact scoreboard operator and edge case still needs a test matrix.

## Update multiple NBT paths

NBT paths can select multiple target nodes. This one command changed three fields:

```mcfunction
data modify storage mdl:bulk items[].n set value 7
```

Given:

```snbt
[{n:0},{n:0},{n:0}]
```

the result was:

```snbt
[{n:7},{n:7},{n:7}]
```

It succeeded with `max_command_forks=1`, so the direct `data modify` operation did
not use command-context forks.

The same applies to list-valued fields:

```mcfunction
data modify storage mdl:bulk items[].xs append value 42
```

which appended to every matched list.

NBT does not provide native arithmetic, so `items[].n += 1` is not directly
available. Bulk NBT is strongest for set, copy, merge, append/prepend/insert, and
remove operations.

## Multiple sources times multiple targets

One command can combine a multi-target path with a multi-source path:

```mcfunction
data modify storage mdl:bulk groups[].xs append from storage mdl:bulk batch[]
```

With three target lists and a three-element batch, each target received all three
elements. This performs nine logical appends through one native data command.

That gives the compiler valuable algebraic rewrites:

```text
for target in targets:
    for value in batch:
        target.append(value)
```

can become one command when aliases, order, element types, and failure semantics
match `data modify` behavior.

## Native bulk versus execute fan-out

These are not equivalent:

```mcfunction
scoreboard players add @e[tag=bulk] mdl.value 1
```

```mcfunction
execute as @e[tag=bulk] run scoreboard players add @s mdl.value 1
```

Under `max_command_forks=1` with three matching entities:

- the first command incremented all three;
- the second incremented none, then its containing function continued.

Likewise, multi-target forms of `execute store` created contexts and hit the fork
limit, even though a direct multi-match `data modify` did not.

The IR should therefore distinguish:

```text
NativeBulk(op, target_set)
ContextFork(context_set, body)
```

A general loop or `execute` must not obscure an operation that the backend can issue
as a native bulk command.

## Represent three synchronized integers as one value

There is an even stronger optimization when values always change together. Instead
of storing three physical scores, represent them as one shared base plus static or
separately stored offsets:

```text
x = base + dx
y = base + dy
z = base + dz
```

Incrementing all three becomes one increment of `base`. This is ordinary compiler
value sharing, not a Minecraft trick.

Another candidate is mixed-radix packing into one 32-bit score and adding a packed
constant. That is only valid when range analysis proves that no lane carries into
another and that the packed value cannot overflow. Shared-base representation is
safer and should be attempted first.

## What cannot be combined directly

One normal mcfunction line contains one Brigadier command. Macros do not turn a line
into an arbitrary semicolon/newline-separated command list.

There is no known native command for simultaneously performing unrelated operations
such as:

```text
x += 1
y *= 4
z = data.foo
```

A function can contain those three commands, but they remain three sequence
operations. Combining them requires discovering shared structure, changing the
physical representation, or using a command with matching native bulk semantics.

## Compiler passes enabled by this

- loop-to-bulk conversion;
- scoreboard objective vectorization;
- aggregate layout selection;
- wildcard NBT path synthesis;
- append/set/remove batching;
- scalar broadcast recognition;
- synchronized-value sharing;
- context-fork elimination;
- fusion of adjacent identical operations;
- alias and failure-semantics checks before fusion.

## Performance warning

One sequence operation does not mean constant wall-clock cost. Minecraft still has
to visit every selected score holder, NBT path, list element, entity, or block
internally. Native bulk operations reduce command parsing, dispatch, and hard-limit
usage, but their wall time must be benchmarked as a function of target count and
data size.

## Remaining tests

- all scoreboard operation operators with `*` and selectors;
- absent scores and empty target sets;
- multi-path `merge`, `remove`, `prepend`, and `insert`;
- source/target aliasing in bulk NBT operations;
- heterogeneous and large NBT elements;
- exact command success/result values for partial/no changes;
- wall time versus equivalent explicit commands at multiple batch sizes;
- selector scan cost for direct scoreboard targeting;
- whether specialized layouts remain worthwhile after conversion costs.

## Primary source

- [Mojang's multi-match NBT path and data operation specification](https://feedback.minecraft.net/hc/en-us/articles/360018363512-Minecraft-Java-Edition-Snapshot-18W43C)

