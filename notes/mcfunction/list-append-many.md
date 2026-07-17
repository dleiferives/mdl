# How Do We Append Ten Things to a List?

## Best case: the destination is still under construction

Construct or set the complete list once:

```mcfunction
data modify storage mdl:heap values set value [1,2,3,4,5,6,7,8,9,10]
```

This is valid only when replacing the previous value is semantically correct. It is
the ideal lowering for a fresh list whose intermediate states cannot be observed.

## Existing destination, batch already stored

Select every element of the source batch and append the matches:

```mcfunction
data modify storage mdl:heap values append from storage mdl:runtime batch[]
```

The `[]` path selects the elements rather than the list tag itself. Without it, the
operation may append a nested list instead of extending the destination.

Status: **Measured** for non-empty homogeneous and heterogeneous batches on vanilla
26.2. An empty source selection leaves the target unchanged and returns
success/result zero. Large-compound performance still requires benchmarks.

## Existing destination, ten literals

Stage and bulk-append:

```mcfunction
data modify storage mdl:runtime batch set value [1,2,3,4,5,6,7,8,9,10]
data modify storage mdl:heap values append from storage mdl:runtime batch[]
```

The compiler can reuse or remove the batch temporary according to liveness. This is
two command lines rather than ten, but actual performance still needs measurement:
the native command may perform ten internal insertions and NBT copies.

## Compiler optimization: append coalescing

Within a region where no code observes the destination between appends:

```text
append xs a
append xs b
append xs c
```

can become:

```text
batch = [a, b, c]
append_range xs batch
```

This requires alias and effect analysis. Coalescing is invalid if a call, raw
command, capture, or read can observe an intermediate list state.

For dynamically produced elements, the compiler can accumulate them in a temporary
batch and flush once. Whether this wins depends on batch size, element size, and the
cost of constructing the batch.

## Prefer bulk path operations over loops

Some apparent list loops are really bulk NBT operations. A uniform update such as
removing the same field from every compound may be expressible with a wildcard path
in one command. The IR should contain operations such as `append_range`, `set_each`,
and `remove_each` long enough for the Minecraft backend to recognize them.

Vanilla 26.2 also accepted multiple target lists and multiple source elements at
once:

```mcfunction
data modify storage mdl:bulk groups[].xs append from storage mdl:bulk batch[]
```

With three `groups` and a three-element `batch`, this produced three appended
elements in each of three lists: nine logical appends from one command, without
hitting a fork limit of one.

## Required tests

- large nested-element copy cost;
- batch size and element-size scaling;
- ten individual appends versus staged bulk append;
- newly constructed list versus repeated mutation.


## Source

- [Mojang's `data modify` list operation announcement](https://www.minecraft.net/pl-pl/article/village---pillage-out-java-)
