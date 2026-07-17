# Native Value Carriers and Command Domains in Java 26.2

## Scope and authority

This note inventories the value forms a data pack can use without a compiler-owned
runtime. The target is the official vanilla Java 26.2 server, data-pack format
107.1. It distinguishes two ideas that should not be collapsed:

- a **carrier** can retain or transport a runtime value; and
- a **command domain** is typed syntax accepted by Brigadier, but may not be
  storable or first-class.

The command-domain inventory came from Mojang's generated `commands.json` report:

```sh
java -DbundlerMainClass=net.minecraft.data.Main \
  -jar minecraft-server-26.2.jar --reports --output reports
```

That report contains 55 distinct argument parsers. Server observations below were
made on a separate vanilla 26.2 world bound to `127.0.0.1:25585`.

## General-purpose carriers

| Carrier | Runtime domain | Strong operations | Weaknesses |
| --- | --- | --- | --- |
| Compiler constant | Any statically known source value | Folding, specialization, direct syntax emission | No runtime mutation |
| Score | Signed 32-bit integer per holder/objective | Arithmetic, comparison, swap, selector/wildcard bulk | Integers only; wrapping; global holder namespace |
| NBT numeric | Byte, short, int, long, float, double | Copy, serialization, scaled command-result conversion | No native arithmetic |
| NBT string | Java string | Copy, slice through `data modify ... string`, macro syntax; indirect exact equality through no-op overwrite | No direct comparison or concatenation syntax |
| NBT array | Byte, int, or long array | Indexed/list-like modification and copying | Homogeneous numeric family; conversion details matter |
| NBT list | Ordered heterogeneous values in 26.2 | Static index, filter path, append/prepend/insert, bulk paths | Dynamic index requires specialization, dispatch, or macro syntax |
| NBT compound | String-keyed value tree | Static key access, recursive merge, set/remove, subset matching; indirect exact equality through no-op overwrite | No native key enumeration or direct source comparison syntax |
| Command outcome | Success plus signed integer result per command context | `execute store`, `return`, conditional control flow | Ephemeral; command-specific meaning |
| Execution context | Executor, position, rotation, dimension, anchor, multiplicity | `execute` transformation and entity-local operations | Not a normal stored value |

The physical NBT carriers can live in command storage, block-entity data, or entity
data. Command storage is the clean compiler-owned heap candidate because it is not
tied to a loaded block or entity. Block and entity NBT are externally observable
world state and have command-specific write restrictions.

Bossbar `value` and `max` are additional integer cells, but they are UI/world
resources rather than a good general register file. Entity tags, teams, block
states, item stacks/components, advancements, predicates, and scheduled functions
also retain state, but their semantics are specialized and should not be treated as
untyped variables.

## NBT value inventory

SNBT in 26.2 exposes these runtime shapes:

```text
signed byte     i8
signed short    i16
signed int      i32
signed long     i64
float           f32-like
double          f64-like
string          Java UTF-16 string semantics at command boundaries
byte array      [B; ...]
int array       [I; ...]
long array      [L; ...]
list            ordered, heterogeneous through the in-game abstraction
compound        string-keyed values
```

The binary NBT end marker is structural, not a user value to expose as an MDL type.
Java 1.21.5 made SNBT and `/data` lists heterogeneous while leaving the binary NBT
file format unchanged; the game transparently wraps values when necessary.

Measured on 26.2:

- one compound retained every numeric width, all three array types, a Unicode
  string, a heterogeneous list, and a nested compound;
- `data get` returned list length and compound entry count as its integer result;
- `data get` returned `4` for `"hé🙂"`: the supplementary character counts as two
  UTF-16 code units;
- slicing the two code units containing `🙂` reproduced it, while slicing only one
  surrogate produced `"?"`;
- appending `128` to a byte array stored `-128b`, and appending `3.5f` stored `3b`;
  appending a string failed. Numeric array insertion therefore performs narrowing
  conversion in the tested cases rather than requiring identical SNBT suffixes.

MDL should still use typed homogeneous `List<T>` by default. Minecraft accepting a
heterogeneous list is a representation capability, not a reason to discard source
typing. A deliberate `Nbt` sum type can expose raw heterogeneity later.

## Command outcomes are a real pair of values

Every executed command has logically separate outcome channels:

```text
success: Boolean-like, possibly aggregated over contexts
result:  signed integer whose meaning depends on the command
```

Mojang's command tree permits either channel to be stored into:

- a block, entity, or storage NBT numeric tag as byte/short/int/long/float/double
  with a scale;
- one or many scores; or
- a bossbar's `value` or `max`.

`return`, `return run`, and `return fail` propagate or construct these outcomes.
A successful function may return integer `0`; result zero and command failure are
therefore not interchangeable. Compiler IR should retain `success` and `result`
separately until a source operation explicitly converts one.

## The 55 command argument domains

These are the exact parser identities in the 26.2 report, grouped by semantic role.
They are candidates for typed source APIs, not automatically runtime representations.

| Role | Parser domains |
| --- | --- |
| Brigadier scalars | `bool`, `double`, `float`, `integer`, `string` |
| Coordinates/context | `block_pos`, `column_pos`, `vec2`, `vec3`, `rotation`, `entity_anchor`, `dimension`, `heightmap`, `swizzle` |
| Ranges/time | `int_range`, `float_range`, `time` |
| Entities/identity | `entity`, `game_profile`, `uuid` |
| Scoreboard/team | `score_holder`, `objective`, `objective_criteria`, `operation`, `scoreboard_slot`, `team`, `team_color` |
| Structured values | `nbt_tag`, `nbt_compound_tag`, `nbt_path`, `component`, `style`, `message`, `hex_color` |
| Blocks/items | `block_state`, `block_predicate`, `item_stack`, `item_predicate`, `item_slot`, `item_slots` |
| Resources | `resource_location`, `resource`, `resource_key`, `resource_or_tag`, `resource_or_tag_key`, `resource_selector` |
| Data-pack behavior | `function`, `loot_table`, `loot_predicate`, `loot_modifier`, `dialog` |
| Other closed domains | `gamemode`, `particle`, `template_mirror`, `template_rotation` |

The names above omit the common `minecraft:` or `brigadier:` prefix for readability.
Some parser instances further require one versus many entities or score holders.
That cardinality is part of the type contract, not merely validation metadata.

## Representation consequences for MDL

1. Keep source meaning independent from physical carrier. `Int32` may be a constant,
   score, NBT int, macro literal, or command result at different program points.
2. Treat scoreboard and NBT conversion as explicit costed edges. NBT has more widths;
   scoreboard arithmetic has only wrapping `i32`.
3. Keep semantic handles such as `EntityQuery`, `Position`, `ResourceId<Item>`, and
   `Text` out of the ordinary aggregate heap unless a use forces materialization.
4. Preserve aggregate intent (`ListMap`, `AppendRange`, `CompoundMerge`) until the
   backend can choose native bulk, scalarization, specialization, or a loop.
5. Model macro syntax values as validated encodings such as `MacroNbtIndex` and
   `MacroResourceId<Function>`, never as arbitrary strings.
6. Raw commands are observation barriers for compiler-owned storage paths and split
   score/NBT representations unless visibility is proven closed.

## Primary sources

- [Minecraft Java 26.2 and data-pack format 107.1](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
- [Heterogeneous SNBT and `/data` lists in Java 1.21.5](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-5)
- [Function macros and their argument compounds](https://www.minecraft.net/en-us/article/minecraft-snapshot-23w31a)
- [`return run`, success, and result behavior](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-3)
- [`data modify ... string` slicing](https://feedback.minecraft.net/hc/en-us/articles/13987663727757-Minecraft-Java-Edition-1-19-4)
