# Typed Execute Chains

Status: **Java 26.2 syntax inventoried; Stage 7.5 modifier subset implemented and
measured**

Implementation plan: [`../compiler/stage-7-5-plan.md`](../compiler/stage-7-5-plan.md).

## Question

How should MDL expose Minecraft's ordered `execute` modifiers so they are typed,
composable, attributable to a multi-statement block, and honest about context forks
and command outcomes?

## Evidence

The official Java 26.2 server JAR was run in data-generator mode on 2026-07-14:

```sh
/opt/homebrew/opt/openjdk@25/bin/java \
  -DbundlerMainClass=net.minecraft.data.Main \
  -jar minecraft-server-26.2.jar \
  --reports
```

Its `generated/reports/commands.json` contains fourteen direct branches under
`execute`:

```text
align anchored as at facing if in on positioned rotated run store summon unless
```

The report is authoritative for accepted Brigadier syntax and argument parser
shapes on the pinned server. It is not a complete semantic contract: it does not by
itself state all context changes, fork behavior, outcome aggregation, or runtime
cost. Those facts require Mojang behavior notes, implementation inspection when
necessary, and real-server boundary tests.

## Complete root inventory

### Context transforms

| Branch | Forms in the 26.2 report | Required typed operands |
| --- | --- | --- |
| `align` | axes | nonempty `Axes` swizzle |
| `anchored` | anchor | `EntityAnchor` (`eyes` or `feet`) |
| `at` | entity targets | `EntityQuery`; can create several contexts |
| `facing` | position; entity targets + anchor | `Position`, or `EntityQuery` + `EntityAnchor` |
| `in` | dimension | `Dimension` |
| `positioned` | position; `as` entity targets; `over` heightmap | `Position`, `EntityQuery`, or `Heightmap` |
| `rotated` | rotation; `as` entity targets | `Rotation` or `EntityQuery` |

These modifiers do not all change the same fields. For example, `at` changes the
dimension, position, and rotation to each selected entity; `positioned` and
`rotated` are narrower. The compiler descriptor must record a `ContextMask` read and
write set per overload instead of assigning the whole family one behavior.

Entity-accepting forms use the server's multiple-entity parser. Their exact fork and
empty-selection behavior must be retained and tested rather than inferred from the
word “context.”

### Executor transforms

| Branch | Forms | Cardinality consequence |
| --- | --- | --- |
| `as` | entity targets | one child context per selected entity |
| `on` | `attacker`, `controller`, `leasher`, `origin`, `owner`, `passengers`, `target`, `vehicle` | relation-specific; `passengers` can be many, the others are at most one |

`as` changes the executor but does not mean “run at that entity.” A common source
chain therefore deliberately writes both `.as(players)` and `.at_executor()`. The
latter is explicit source sugar for using Minecraft's current `@s`; it does not
inject a variable named `self`.

`on` is not a query detached from context. It follows a relation from the current
executor and binds each result as the new executor. A missing relation produces no
child context. Relation and cardinality belong in a closed `EntityRelation`
descriptor.

### Stateful executor creation

`summon <entity type>` creates an entity at the current context and binds it as the
new executor for the rest of the chain. It is both a world mutation and a context
transition. It cannot be treated like a pure builder setter even though its source
shape is another method.

### Filters

Both `if` and `unless` accept the same condition families:

| Condition | Reported forms |
| --- | --- |
| biome | block position + biome resource or tag |
| block | block position + block predicate |
| blocks | source region + destination + `all` or `masked` |
| data | block/entity/storage source + NBT path |
| dimension | dimension |
| entity | entity query existence |
| function | function resource |
| items | block/entity holder + slots + item predicate |
| loaded | block position |
| predicate | loot predicate resource |
| score | score comparison, or integer range match |
| stopwatch | resource ID + floating-point range |

A chain of conditions is conjunction in source order. `unless` negates one
condition; it is not a general Boolean `else`. Some conditions are observational,
while `if function` executes nested work and has command-result semantics. A single
`Condition` surface type therefore needs a behavior summary, not only a rendered
predicate string.

### Outcome stores

`store` first selects `result` or `success`, then one destination:

| Destination | Remaining shape |
| --- | --- |
| block NBT | position, path, numeric tag type, scale |
| bossbar | resource ID, `max` or `value` |
| entity NBT | entity target, path, numeric tag type, scale |
| score | score holder and objective |
| storage NBT | resource ID, path, numeric tag type, scale |

The numeric NBT types are `byte`, `double`, `float`, `int`, `long`, and `short`.
`result` and `success` are different channels. A store observes the nested terminal
command after it finishes; it is not an assignment performed when the builder method
is called.

An arbitrary multi-statement language block does not inherently have one Minecraft
command result. The typed API must therefore reject a `run.store_*() { ... }` scope
unless the block has an explicit outcome contract. Accidentally storing whatever
result an outlined `function` happens to return would expose a lowering detail as
source semantics.

### Terminal run

Minecraft's terminal `run` redirects into another command. MDL moves that word to
the front of one contextual block statement:

```text
run.as(entities).at_executor() |entity| {
    statement_1;
    statement_2;
}
```

The compiler may emit a one-statement body directly or outline a multi-statement body
to a generated function. That choice is not source-visible and must preserve
context, captures, multiplicity, returns, and cost. The leading `run` is not an
ordinary function or value. A future deferred command value could justify a second
form, but Stage 7 does not create a command-object type merely to mirror Brigadier.

## Selected MDL model

Use a non-first-class compiler-known contextual statement with a fluent working
syntax:

```text
if (enabled) run
    .as(players)
    .at_executor()
    .anchored(eyes)
    .rotated(rotation)
    .in(overworld)
    .if(is_loaded(position)) |player| {
        player.say("hello");
        update_state();
    }
```

The exact tokens remain revisable. In particular, keyword member names such as
`.as`, `.in`, and `.if` are a parser policy, not an IR requirement.

Conceptually the parsed statement accumulates:

```text
RunScope {
  ordered_modifiers
  resulting_context
  invocation_bound
  effects
  terminal_outcome_requirement
  origin
}
```

This is compile-time structure represented by one HIR region. It has no runtime
Minecraft representation, cannot be placed in NBT, and disappears when its region
lowers. The grammar makes it non-first-class: `run`, its ordered method calls, and its
block are one statement and cannot be assigned, passed, or returned.

The optional Zig-style capture binds only a proven final executor:

```text
run.as(players).at_executor() |player| {
    player.say("hello");
}
```

`player` is one per-invocation `Executor<Player>` capability. It proves that the
current Minecraft `@s` has that type; it is not a selector, UUID, or stored entity
handle. Stage 7 permits it as a method receiver but does not let it escape or pass as
an ordinary value. A context-only modifier such as `positioned` does not manufacture
an executor capture, and `at` does not bind the entity whose context it copied.

The region is structural. Do not encode it as flat `enter_context`, body,
`leave_context` instructions: branches, early returns, DCE, outlining, and scheduled
yields would make restoration unsound.

A multi-statement body is outlined once and invoked by one contextual external Core
operation. Repeating the execute chain separately for every statement is incorrect
under forks: it changes per-entity `A; B` into all-entities `A` followed by
all-entities `B`. The outlined function is a real call-graph/reachability edge even
though its invocation carries Minecraft context semantics.

## Relationship to entity methods

An entity method is concise sugar over one receiver-scoped operation:

```text
player.say("hello")
```

means that the exact-one `player` entity executes `say`. It does not send a message
to that player. The target recipe is semantically equivalent to the one-statement
scope:

```text
run.as(player) |executor| { executor.say("hello"); }
```

The compiler may select the shortest verified structured recipe without first
constructing the surface sugar.

The official server report gives `say` a `minecraft:message` argument. The first
slice therefore uses a compile-time `MessageLiteral`, not the future structured JSON
`Text` type. Those types can both accept convenient source literals while remaining
semantically distinct.

Receiver role is command-specific. `EntityRef.say` establishes its receiver as the
executor and then uses the same ambient `Say` operation as `Executor.say` inside a
run capture. `EntityRef.teleport`, by contrast, uses its receiver as the teleport
target. Dotted syntax must not blindly insert `execute as` for every entity method.

The first runnable slice does not require a general physical `EntityRef`. A static
tagged query with `limit(1)` has `AtMostOne` cardinality; when its run body is entered,
the explicit capture is an exact executor capability. An empty query skips the body.
This gives Stage 7 a real server proof while runtime entity handles remain honest
Stage 8 work.

A possibly-many query does not masquerade as a scalar receiver:

```text
players.say("hello")                              // rejected
run.as(players) |player| { player.say("hello"); } // explicit fork
```

The second spelling exposes Minecraft's multiplicity at the source level. Under the
current fixed-slot single-context ABI, Stage 7 may model and diagnose that fork but
initially lower only at-most-one outlined invocations. General forked bodies belong
with the Stage 8 calling convention and Stage 9 work analysis.

## Real-server coverage and remaining inventory

Stage 7.5 now measures every selected modifier (`as`, `at`, `at_executor`,
`positioned`, `rotated`, `in`, `anchored`, and `align`) plus teleport/move-by
semantics. The broader execute inventory below remains required when those deferred
families enter the typed language:

1. Empty, one, and several matches for every entity-accepting modifier.
2. Which context components each transform preserves or replaces, including
   cross-dimension entities.
3. Cardinality for every `on` relation, especially `passengers`.
4. Context after `summon` and failure behavior for invalid/unavailable entity types.
5. Success/result aggregation through filters, forks, function conditions, stores,
   early returns, and zero-child chains.
6. Multi-statement outlining equivalence for nested modifier sequences.
7. Command-sequence and fork-limit accounting for every modifier class.

## Primary references

- Generated `commands.json` from the official Minecraft Java 26.2 server JAR
- [Minecraft snapshot 17w45a: execute chaining, conditions, and stores](https://www.minecraft.net/sv-se/article/minecraft-snapshot-17w45a)
- [Minecraft snapshot 18w02a: facing, positioned, rotated, in, at, and anchored](https://feedback.minecraft.net/hc/en-us/articles/360004167991-Minecraft-Java-Edition-Snapshot-18W02A)
- [Minecraft Java 1.19.4: on, positioned over, summon, dimension, and loaded](https://feedback.minecraft.net/hc/en-us/articles/13987663727757-Minecraft-Java-Edition-1-19-4)
- [Minecraft snapshot 25w41a: stopwatch condition](https://feedback.minecraft.net/hc/en-us/articles/40290141596301-Minecraft-Java-Edition-Snapshot-25w41a)
- [Minecraft Java 26.2: current `execute on owner` behavior change](https://feedback.minecraft.net/hc/en-us/articles/46690753273997-Minecraft-Java-Edition-26-2)
