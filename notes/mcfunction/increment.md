# How Do We Increment a Value?

## Scoreboard-backed integer

One command:

```mcfunction
scoreboard players add #value mdl.reg 1
```

This is the baseline representation for frequently mutated integers.

Fixed-point values are equally simple. If scale 1000 represents three decimal
places, incrementing by `1.0` adds 1000:

```mcfunction
scoreboard players add #value mdl.reg 1000
```

## NBT-backed integer

NBT has storage but no direct arithmetic operation. The ordinary baseline is a
scoreboard round trip:

```mcfunction
execute store result score #tmp mdl.reg run data get storage mdl:heap object.count
scoreboard players add #tmp mdl.reg 1
execute store result storage mdl:heap object.count int 1 run scoreboard players get #tmp mdl.reg
```

That is a strong signal for representation selection: if a field is incremented
often, scalarize it into a scoreboard and write it back to NBT only when an external
consumer needs the serialized structure.

## Increment several values together

Minecraft can increment globally tracked score holders with one native command:

```mcfunction
scoreboard players add * mdl.vector 1
```

This also creates the objective score for holders tracked through other objectives,
so a dedicated objective is not sufficient to define vector membership. Entity-backed
values can instead use a selector directly. These are true bulk scoreboard operations
and are developed in [bulk-operations.md](bulk-operations.md).

## Compiler rule

Track mutation patterns before choosing storage:

```text
mostly arithmetic       -> scoreboard
mostly bulk copy/NBT IO -> command storage
mixed hot/cold struct   -> scalarize hot fields, retain cold NBT fields
compile-time value      -> constant
```

A struct can therefore have a split physical representation while remaining one
typed source value.

## Eliminate increments when possible

Normal compiler transformations matter greatly:

- fold increments into a later comparison range;
- combine adjacent increments into one `add N`;
- remove increments of dead values;
- use induction-variable analysis for loops;
- replace a counter with native command result/fork count when semantics match;
- defer a write-back until the NBT form is actually observed.

## Overflow semantics

Vanilla 26.2 score arithmetic is measured wrapping signed 32-bit arithmetic:
`2147483647 + 1` became `-2147483648`, `-2147483648 - 1` became `2147483647`, and
`50000 * 50000` became `-1794967296`. `INT_MIN / -1` also wrapped to `INT_MIN`.

The language must still choose whether source integer overflow wraps, traps,
saturates, or is statically rejected. If it chooses another policy, lowering must
insert checks and the optimizer must preserve them.

## Required tests

- checked and saturating arithmetic recipes;
- exact command results for multi-target overflow;
- NBT byte/short/int/long write-back overflow;
- increment fusion across branches and raw commands;
- split struct field synchronization;
- scoreboard versus NBT round-trip cost.
