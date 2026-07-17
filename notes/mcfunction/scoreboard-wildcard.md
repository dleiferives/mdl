# What Exactly Does Scoreboard `*` Mean?

## Short answer

Scoreboard `*` is one exact reserved token meaning:

```text
take a snapshot of every holder that currently has at least one score anywhere
in the global scoreboard service
```

It is not a glob or regular expression. These are all literal holder names:

```text
#group*
#group.*
#group?
#group[ab]
group*
```

Only a token exactly equal to `*` requests wildcard expansion. There is no escape
for addressing a literal holder whose complete name is `*` through this argument.

Status: **Source-inspected and measured** on the official vanilla Java 26.2 server.

## Parser behavior

The official `ScoreHolderArgument` class performs these steps:

1. If the first character is `@`, parse an entity selector.
2. Otherwise consume one token up to the next space.
3. If the complete token equals `*`, return the wildcard result.
4. Otherwise create one literal score-holder name.
5. A name beginning with `#` remains literal without player-name or UUID lookup.
6. Other names may resolve to an online player or loaded entity before falling back
   to a name-only holder.

This gives compiler-owned `#...` fake holders a useful stability property. Prefixes,
suffixes, regex characters, quoted strings, and wildcard-looking fragments receive
no pattern semantics.

Measured values proved the distinction: after creating `#grp.alpha`, `#grp.beta`,
`#grp*`, `#grp?`, `#grp[ab]`, and `#grp.*`, adding to `#grp*` changed only that one
literal holder. Adding to `#grp.*` likewise changed only that exact name.

## What is a tracked holder?

The 26.2 scoreboard owns one global map:

```text
holder name -> that holder's objective/score map
```

Wildcard expansion uses the outer map's key set. A holder is therefore tracked when
it has at least one score in any objective. It is not enough to belong to a team.
Removing the holder's last score removes it from this set.

Consequences:

- offline players with retained scores, fake holders, and live entity-backed scores
  can all participate;
- a holder with a score only in objective `A` is still selected by `*` while a
  command writes objective `B`;
- setting, adding, displaying, enabling, storing, or operating on `* B` commonly
  creates `B = 0` or another value for every selected holder; and
- team-only names are invisible to `*` until they acquire a score.

The game removes all scores and team membership for a removed non-player entity.
Player/fake-holder persistence has different lifetime semantics and should not be
treated as a loaded-entity selector.

## Commands that expand `*`

The official 26.2 command implementations opt into the global wildcard set here:

| Command position | Wildcard effect |
| --- | --- |
| `scoreboard players set/add/remove * OBJ ...` | Mutate every tracked holder; create missing `OBJ` scores |
| `scoreboard players reset * [OBJ]` | Remove one objective or every score from every tracked holder |
| `scoreboard players operation * ...` target | Repeatedly update every tracked holder |
| `scoreboard players operation ... * ...` source | Feed every tracked holder to each target operation |
| `scoreboard players display name * OBJ ...` | Create missing `OBJ = 0` scores and change display metadata |
| `scoreboard players display numberformat * OBJ ...` | Create missing `OBJ = 0` scores and change formatting |
| `scoreboard players enable * TRIGGER` | Create/unlock trigger scores for every tracked holder |
| `execute store result|success score * OBJ ...` | Store one command outcome into the captured holder set |
| `team join TEAM *` | Add every score-tracked holder to the team |
| `team leave *` | Remove every score-tracked holder from teams |

These single-holder positions parse `*` but cannot expand it and fail with “No
relevant score holders could be found”:

```text
scoreboard players get * OBJ
scoreboard players list *
execute if|unless score * ...
execute if|unless score ... * ...
```

The no-argument `scoreboard players list` is a separate global enumeration command.
Its integer result is the tracked-holder count; the measured two-holder case stored
result `2`.

An empty wildcard expansion fails before the operation. On a fresh scoreboard,
`players add * OBJ 1` stored success/result zero and created no holders.

## Useful computational patterns

### Broadcast a literal or scalar

```mcfunction
scoreboard players set * mdl.vector 7
scoreboard players operation * mdl.vector = #scalar mdl.constants
scoreboard players operation * mdl.vector += #delta mdl.constants
```

The same shape supports wrapping arithmetic and min/max clamps:

```mcfunction
scoreboard players operation * mdl.vector < #upper mdl.constants
scoreboard players operation * mdl.vector > #lower mdl.constants
```

This is only a valid logical vector when the whole global tracked-holder set is the
intended membership set. A dedicated objective does not establish that property.

### Broadcast a command outcome

```mcfunction
execute store result score * mdl.result run <command>
execute store success score * mdl.success run <command>
```

The holder set is captured before the final command runs. In the measured test, the
run command created `#new`, but only pre-existing `#old` received the stored result.

### Reduce over all tracked holders

```mcfunction
scoreboard players operation #acc mdl.out += * mdl.input
```

With source values `2` and `3` and all other tracked holders materializing as zero,
the accumulator moved from `100` to `105`. Related candidates are:

- `+=` for a wrapping sum;
- `-=` for subtracting a wrapping sum from one accumulator;
- `*=` for a wrapping product;
- `<` for minimum; and
- `>` for maximum.

Every tracked holder participates, including the accumulator if it is tracked. The
accumulator's source-objective value must therefore be an appropriate identity.
Missing source-objective scores are materialized as zero, which breaks product and
can distort min/max. There is no way to request only “holders already present in
`mdl.input`.”

### Clear scoreboard state

```mcfunction
scoreboard players reset * mdl.temporary
scoreboard players reset *
```

Objective-specific reset removes that score without creating it for missing holders.
Holders with no scores left stop being tracked. Full reset removes all score state
but does not remove team membership: the server retained team members after they
disappeared from `scoreboard players list`.

The reset command result is the size of the captured global holder set, not the
number of values that actually existed in the selected objective.

### Bulk trigger, display, and team initialization

`display name`, `display numberformat`, and `enable` all expand the same global set
and create missing zero scores. Team wildcard operations use score tracking as their
membership source. A measured team contained one team-only member plus eleven
score-tracked holders; `team leave *` removed the eleven and retained the team-only
member.

These are potentially useful administrative primitives, but they are not a hidden
way to query team members back into scoreboard operations.

## Live iteration and alias hazards

Wildcard collections are snapshots of holder identity, not snapshots of score
values. `scoreboard players operation` loops targets outside and sources inside,
reading and writing live scores for every pair.

With only `#a = 1` and `#b = 2`:

```mcfunction
scoreboard players operation * star_alias += * star_alias
```

produced:

```text
#a = 4
#b = 12
```

The first target changed values subsequently read while processing the second. The
holder map is an open hash map, so iteration order is not a semantic ordering MDL
can expose. `* op *` with aliased objectives is therefore not a stable vector or
matrix primitive.

Non-aliased multi-source operations can also fail partway without rollback. Starting
with target `100` and wildcard sources `2` then `0`, `/=` first changed the target to
`50`, then failed on division by zero. The command reported failure/result zero but
left the partial mutation in place.

Safe baseline rules are:

- require at-most-one source for ordinary scalar lowering;
- require disjoint source/target objectives for intentional reductions;
- restrict reductions to an operation with proven order independence;
- prove every tracked holder has the required source score and identity value; and
- reject any recipe whose failure could expose a partial update.

## Can regex-like selection be built another way?

Not natively over fake-holder names. Practical alternatives are:

1. **Entity-backed holders:** use selectors with exact tags, types, score ranges,
   teams, and other typed filters. This only addresses selectable entities.
2. **Generated explicit holders:** emit one command per statically known fake holder.
3. **Compiler-owned registry:** keep exact holder names in an NBT list and iterate
   through a safely encoded macro command.
4. **Finite group dispatch:** specialize a known group/enum into ordinary functions.
5. **Different layout:** use an NBT list/compound or entity query when runtime group
   membership matters more than direct scoreboard arithmetic.

Naming fake holders with a shared prefix is useful for debugging and collision
avoidance, but it creates no runtime selection capability.

## Compiler follow-up

The current structured IR renders `ScoreHolders::AllTracked` correctly as `*`, but
its Rust documentation describes “every holder currently tracked by the objective.”
The actual 26.2 contract is global across all objectives. When implementation edits
resume, audit that documentation and any analysis that assumes objective-scoped
membership; do not change rendering merely because the comment is inaccurate.

The compiler should treat global wildcard access as a world-visible scoreboard
effect, not an objective-local vector operation. Raw commands and other data packs
can add or remove participating holders.

## Reproduction authority

- Official Java 26.2 `commands.json` generated through `net.minecraft.data.Main`;
- official `ScoreHolderArgument`, `ScoreboardCommand`, `ExecuteCommand`,
  `TeamCommand`, and `Scoreboard` classes inspected from the 26.2 server JAR; and
- isolated vanilla server experiments on `127.0.0.1:25585` in a fresh world.

Relevant Mojang release notes:

- [Minecraft Java 26.2](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
- [Score display metadata additions](https://feedback.minecraft.net/hc/en-us/articles/21396856976141-Minecraft-Java-Edition-Snapshot-23w46a)
- [`execute store`, command result, and function behavior](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-3)
