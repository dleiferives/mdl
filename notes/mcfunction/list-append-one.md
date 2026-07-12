# How Do We Append One Element to a List?

## Literal element

Use the native list operation:

```mcfunction
data modify storage mdl:heap values append value {name:"cell",occupied:false}
```

Status: **Documented**. Mojang introduced `insert`, `prepend`, and `append` as
`data modify` list operations.

## Element already stored as NBT

Copy it directly:

```mcfunction
data modify storage mdl:heap values append from storage mdl:runtime value
```

This logically copies the NBT value into the list. Large compounds may therefore
have a materially different cost from small scalar elements even though both are
one command line.

## Element currently held in a scoreboard

Bridge it into an NBT numeric tag, then append:

```mcfunction
execute store result storage mdl:runtime value int 1 run scoreboard players get #value mdl.reg
data modify storage mdl:heap values append from storage mdl:runtime value
```

A one-line macro candidate is:

```mcfunction
$data modify storage mdl:heap values append value $(value)
```

if `value` is already a macro argument with the required NBT type. The compiler must
benchmark macro instantiation against the ordinary two-command bridge. One line is
not automatically faster.

## When append should disappear

If the list is newly allocated and its elements are all known in the same region:

```mdl
let xs = []
xs.push(a)
xs.push(b)
xs.push(c)
```

should become one list construction, not three appends. The IR must distinguish
observable mutation from a construction sequence that can be aggregated.

Likewise, appending and immediately reading only the appended element should often
forward the value without reading the list again.

## Type constraints

MDL list element types should be stricter than raw NBT. Even where modern Minecraft
accepts heterogeneous NBT lists, `List<T>` should normally guarantee one source
type. Explicit `List<Nbt>` or a sum type can opt into heterogeneity.

The backend must also know whether the destination path definitely exists as a list.
Initialization and append failure are observable command behavior.

## Required benchmarks

- literal versus `from` source;
- scalar versus large compound element;
- scoreboard bridge versus macro append;
- appending at end versus inserting at front/middle;
- hot-list size sensitivity;
- behavior when the destination is absent or empty.

## Source

- [Mojang's `data modify` list operation announcement](https://www.minecraft.net/pl-pl/article/village---pillage-out-java-)

