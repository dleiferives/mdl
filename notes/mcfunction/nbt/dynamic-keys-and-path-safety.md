# Dynamic Compound Keys and NBT-Path Safety

A macro can place a runtime field name into an NBT path, making direct compound
lookup possible. The hard part is not lookup; it is rendering an arbitrary source
string as exactly one safe NBT-path segment.

## Strings and path segments are different types

Macro string values are substituted as raw characters and the resulting line is
parsed as a command. Therefore this is unsafe as a generic dictionary operation:

```mcfunction
$data modify storage example:data map.$(key) set value $(value)
```

A dot, quote, backslash, bracket, or whitespace in `key` can change the command
grammar. Wrapping `$(key)` in a fixed quote pair is also incomplete: the key may
contain that quote or an escape sequence.

MDL needs an internal distinction:

```text
String --encode_nbt_path_segment--> MacroNbtKey
```

Only `MacroNbtKey` may be spliced into a path. The encoder chooses a valid quoted
SNBT/path form and escapes the chosen delimiter and backslashes.

## Measured special-key matrix

The 26.2 fixture supplied already encoded path-segment strings to one macro line.

| Logical key | Encoded segment form | Result |
| --- | --- | --- |
| `alpha` | `alpha` | stored `1` |
| `space key` | `"space key"` | stored `2` |
| `a.b` | `"a.b"` | stored `3` as one field, not a path |
| `a"b` | an escaped quoted segment | stored `4` |
| `a\b` | an escaped quoted segment | stored `5` |
| `123` | `"123"` | stored `6` as a string key |
| empty string | `""` | instantiated command failed; no field/status was written |

An additional static probe containing `{"":7}` made the entire function fail to
load with a parser error on 26.2. It was removed from the runnable fixture after the
failure was recorded. Empty NBT names can exist in the binary format and appear in
Minecraft's heterogeneous-list encoding, but current command SNBT/path syntax does
not give a usable ordinary dictionary-key operation for them.

Consequences for `Dict<String,V>`:

- reject empty keys with a source-level error;
- encode the empty key to a reserved non-empty physical key with collision rules;
- or select a list-of-entry representation that stores the logical key as a value.

The third choice preserves the complete string domain and arbitrary NBT keys, at
the cost of linear lookup without an auxiliary index.

## Raw paths are an unsafe capability

Several surveyed libraries pass values such as `storage namespace:id some.path`
or user-selected `path` strings and splice them directly into macro commands. This
is convenient for trusted pack authors but combines multiple grammar domains:

- target kind (`storage`, `entity`, or `block`);
- resource or selector identity;
- NBT path; and
- sometimes a complete SNBT value or function ID.

MDL should represent these as structured typed values and render the final command
fragment at the backend boundary. Raw command fragments belong only in an explicit
checked-native/unsafe API.

## Dynamic values need typing too

Non-string macro values are serialized as SNBT. That allowed the fixture to splice
an int array directly into a list-compound filter. It does not make every string a
safe SNBT literal: a string substituted into `set value $(value)` is again parsed
as syntax. `MacroSnbt<T>`, `MacroNbtKey`, `MacroNbtPath`, and ordinary `String`
should be separate IR types.

## Compiler rules

1. Constant field names become static paths; no macro is needed.
2. Pre-encode one runtime key into one path segment; never concatenate unchecked
   user text with dots/brackets/quotes.
3. Keep the macro function as small as possible so only the dynamic command is
   reparsed.
4. Specify missing-key and invalid-key behavior in source semantics.
5. Test both quote forms, backslashes, control characters, Unicode, numeric-looking
   names, and empty strings before claiming a total key encoder.

## Sources

- [Mojang function macro specification](https://www.minecraft.net/en-us/article/minecraft-snapshot-23w31a)
- [stdmodulesystem plain map](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/collections/data/collections/function/map)
- [MCF: Map source](https://github.com/Wisoven/MCF-Map/tree/520fa57cfaf4e5755f0c9e2220d1339e1400e106/data/mcfmap/function)
