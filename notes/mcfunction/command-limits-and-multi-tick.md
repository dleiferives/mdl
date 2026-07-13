# What Are the Command Limits, and How Can Work Span Ticks?

## Verified 26.2 defaults

On a fresh vanilla Minecraft Java 26.2 world:

```text
minecraft:max_command_sequence_length = 65536
minecraft:max_command_forks           = 65536
```

These are the current names. Game rules moved to a registry and were renamed to
namespaced snake-case IDs in 1.21.11:

```text
maxCommandChainLength -> minecraft:max_command_sequence_length
maxCommandForkCount   -> minecraft:max_command_forks
```

So the default is 65,536 rather than approximately 32,000.

Status: **Measured** on the official 26.2 dedicated server on 2026-07-11 and
supported by Mojang's game-rule documentation.

## The sequence limit does not mean “lines in one file”

Mojang defines sequence operations to include, in the ordinary case:

- executing one command for one context;
- executing a stage in an `execute` chain;
- invoking a function.

The limit follows the execution chain across nested function calls. A compact line
can be expensive when it forks, while comments and unexecuted branches cost nothing.

The official deobfuscated 26.2 server implementation has two custom-executor
exceptions that matter to exact accounting. `return <value>` and `return fail` do
not increment the sequence cost themselves. An `execute if/unless function` modifier
is still an execute-stage metric, but the custom modifier does not increment sequence
cost independently; each function invocation it queues still does. Ordinary execute
modifiers increment once even when the current context list is already empty, while
the final nested body runs once per surviving context (or not at all for zero).

The separate fork limit bounds contexts created by commands such as `execute as`.
For five matching entities, `as @e` creates five contexts, while `as @e at @e` can
create 25.

## The 65,536 limit is per root sequence, not per tick

Minecraft does not maintain one shared 65,536-operation allowance for an entire
tick. One root command and all functions, `execute` stages, and contexts descended
from it form a sequence. A separate root command starts a separate sequence, even
when the server processes both roots during the same tick.

This was tested with the limit reduced to 10. A function containing eight score
increments costs nine sequence operations including its invocation, so it fits.
Two function commands were submitted together and processed at the same server log
timestamp:

```text
function mdl:limit/eight
function mdl:limit/eight
#roots = 16
```

If the limit were global to the tick, the second root could not have completed. It
did complete, because each invocation had its own sequence budget.

Consequences:

- one function cannot evade the limit by calling more ordinary functions;
- a scheduled continuation can continue later as a new root;
- separate command blocks are separate roots, as Mojang explicitly documents;
- many independent roots can collectively execute far more than 65,536 operations
  during one tick.

The final point is dangerous: the limit prevents runaway individual command trees,
not general tick lag.

## One scheduled function tag creates multiple roots

There is an important difference between calling and scheduling a function tag.
With `max_command_sequence_length` set to 10, a tag contained two functions that
each attempted 15 score increments followed by a completion marker.

Direct invocation:

```mcfunction
function #mdl:root_limits
```

uses one shared sequence. By contrast, this one command:

```mcfunction
schedule function #mdl:root_limits 1t replace
```

caused both tag members to start as separate roots on the following tick. The server
logged the limit stop twice; both counters reached 9, and neither completion marker
was reached:

```text
Command execution stopped due to limit (executed 10 commands)
Command execution stopped due to limit (executed 10 commands)
#limit_a = 9
#limit_b = 9
```

Functions listed separately in `minecraft:tick` were also observed to receive
separate roots. This gives a static compiler two legitimate root-generation tools:

- permanent, statically listed tick lanes;
- one scheduled tag command that fans out into statically listed roots next tick.

It cannot be used synchronously: `schedule ... 0t` fails with `Can't schedule for
current tick` on 26.2.

This is not free parallelism. The roots still run sequentially on the server thread
and their combined work can make the tick arbitrarily slow.

## Fork-limit behavior is different

The fork gamerule did not behave as a cumulative per-root or per-tick allowance in
the experiments.

With `max_command_forks` set to 4:

- one `execute as` matching six armor stands executed zero bodies, then the function
  continued;
- two separate `execute as` commands matching three stands each both executed all
  three bodies, even in one directly called function tag;
- one combined `execute as` three stands `at` three stands, which would produce nine
  contexts, executed zero bodies.

The final 26.2 server checks `accumulated_output_size + new_size >= fork_limit`.
Therefore a chain expansion must be strictly less than the gamerule: a value of 4
permits at most 3 output contexts at the checked redirect stage. An exact boundary
fixture measured three contexts executing the body three times, while four and five
contexts both rejected the redirect and executed the body zero times.

Configured fork zero is accepted by 26.2. Direct non-redirect commands still execute;
an `execute as` producing one context did not. Values zero and one were
observationally identical for that one-context redirect, while value two allowed it.
Bytecode inspection additionally shows that a checked redirect at zero compares even
a zero-sized result against zero, so a compiler must retain whether a redirect check
is reachable rather than assuming every numeric zero is safe.

There is a target-specific exception: `execute if function` and `execute unless
function` use a custom modifier path that returns before the generic `BuildContexts`
fork guard. At both configured fork zero and one, a true function condition ran its
body once while one-context `as`, `in`, `store result`, and an ordinary passing `if
score` ran no bodies. The compiler must therefore distinguish ordinary fork-checked
redirect expansion from custom function-condition context flow; “execute stage” alone
is too broad to be the fork metric.

Thus the relevant bound is the context expansion of an individual `execute` chain,
not a total fork-token budget accumulated across all commands in a root. Scheduled
roots still give independent command execution, but they are not needed merely to
reset a cumulative fork counter because the observed counter is not cumulative in
that way.

For the compiler, every generated ordinary fork-checked redirect needs a proven or
configured bound on the product of its fork-producing stages. Custom function
conditions still need outcome and sequence accounting, but do not add a generic fork
check of their own. Splitting one large fork into multiple bounded commands can fit
the gamerule, but still consumes the combined runtime work.

## Other mechanisms can stop or punish a huge tick

Minecraft does not automatically spread excess work into later ticks when the
normal 50 ms target for 20 TPS is exceeded. The tick simply takes longer and the
server falls behind.

Other safeguards are independent:

- `minecraft:max_command_forks` limits contexts created by one sequence;
- player-issued command packets are subject to command-spam protection;
- the dedicated-server watchdog can terminate a server after an excessively long
  tick rather than yield and resume it;
- individual commands have their own permission, size, block-modification, entity,
  or syntax constraints.

The fresh vanilla 26.2 `server.properties` generated during testing contained:

```properties
command-spam-threshold-seconds=10
max-tick-time=60000
```

Mojang documents the command/chat spam counters in 26.2: each submitted command adds
one second to the command counter, the counter decreases by 1/20 second per tick,
and the player is kicked when the configured threshold is reached. This protects
network command submission; it is not a datapack instruction budget. Console input
and internally generated datapack work should not be treated as equivalent to player
command packets.

## Local limit experiment

The vanilla server gamerule was temporarily set to 10. A function containing a
score initialization, fourteen increments, and a final storage write was invoked
from the console.

Observed server output:

```text
Command execution stopped due to limit (executed 10 commands)
#direct has 8 [mdl_test]
direct_completed was absent
```

The outer function invocation consumed an operation, followed by the score set and
eight increments. This confirms that source lines are not the correct unit.

The same fourteen increments were divided across three functions connected by
one-tick schedules. Each segment remained under the limit of 10. The final observed
state was:

```text
#multi = 14
multi_completed = true
```

Status: **Measured**. Scheduling the continuation in a later tick begins a new
command sequence and can therefore continue work beyond the original sequence
limit.

## Exact sequence equality and configured zero

With `max_command_sequence_length = 10`, a chain containing nine nonforking
`execute in` stages plus its final scoreboard body completed all ten counted
operations. Adding one more stage stopped before the body. The exact-ten run still
logged the limit-stop message after its final state change, so tests must assert
world state rather than interpreting that message alone as failure.

The configured values zero and one behaved identically: one direct operation ran,
but a two-operation chain stopped after the first operation. Invoking a function used
that sole operation and its body did not run. Java 26.2 therefore executes with an
effective sequence quota of `max(1, configured_value)`.

These boundary fixtures used the extracted official 26.2 server JAR with SHA-256
`183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8` and OpenJDK
25.0.3, without a connected client.

## Compiler configuration: hard limits and soft budgets

The person compiling the project should be able to configure the assumed server
limits:

```toml
[target.minecraft.limits]
max_command_sequence_length = 65536
max_command_forks = 65536

[target.minecraft.budget]
max_operations_per_tick = 1000
max_forks_per_tick = 256
overflow = "split" # "split", "error", or "unchecked"
runtime_check = true
```

The exact configuration syntax is provisional. The distinction is not:

- **hard limits** describe what the target server allows;
- **soft budgets** describe how much work this program is permitted to consume while
  leaving time for Minecraft, players, and other packs.

A compiler should almost never aim to consume all 65,536 operations every tick.

## Do not silently change global game rules

The generated pack should not automatically raise either gamerule. They affect all
commands and all data packs in the world.

Instead, a load-time compatibility check can query the actual values:

```mcfunction
execute store result score #sequence_limit mdl.meta run gamerule max_command_sequence_length
execute store result score #fork_limit mdl.meta run gamerule max_command_forks
```

Both query/store commands were verified on 26.2. The pack can disable itself or emit
a diagnostic when the server provides less than the compiled requirement.

Projects that explicitly own the world may separately offer an opt-in setup command
that changes the rules, but compilation alone should not do so.

## Primitive 1: one-shot scheduled continuation

Current 26.2 syntax is:

```mcfunction
schedule function <function> <time> [append|replace]
schedule clear <function>
```

There is no syntax for scheduling macro arguments or an execution context.

A compiler can lower an explicitly multi-tick operation into states:

```mcfunction
# phase_0.mcfunction
data modify storage mdl:jobs active.locals set value {...}
data modify storage mdl:jobs active.pc set value 1
schedule function mdl:generated/job_42/resume 1t replace
return 0
```

```mcfunction
# resume.mcfunction
execute if data storage mdl:jobs {active:{pc:1}} run function mdl:generated/job_42/phase_1
```

`replace` is the default and prevents more than one pending invocation of that
function ID. `append` allows repeated schedules, but the callback still receives no
per-invocation arguments. Multiple logical jobs therefore need an explicit queue or
unique specialized continuation IDs.

## Scheduled functions lose the caller's execution context

This was tested by invoking the scheduling function `as` an armor stand. The entry
function observed `@s`; the scheduled callback one tick later did not:

```snbt
{start_has_self:1b,resume_has_self:0b,resumed:1b}
```

Therefore a continuation must save every live context component it needs:

- executor identity;
- position;
- rotation;
- dimension;
- anchor;
- local variables and temporary values;
- program counter/continuation ID.

The callback must then resolve or reconstruct that context. Resolution can fail if
an entity was removed, a player went offline, a dimension became unavailable, or
the world changed. These outcomes belong in the language semantics, not in hidden
backend behavior.

## Primitive 2: tick-driven work queue

For multiple jobs, loops, or library operations, a persistent scheduler called from
`minecraft:tick` is more composable than scheduling every logical call directly.

Conceptual job record:

```snbt
{
  kind:"mdl:string_find",
  pc:2,
  executor:[I;...],
  dimension:"minecraft:overworld",
  locals:{start:120,end:124},
  remaining_budget:200
}
```

Each tick, the runtime:

1. replenishes the program's soft budget;
2. takes a job from the queue;
3. runs a statically bounded chunk of work;
4. completes it or writes back its continuation;
5. rotates unfinished work to preserve fairness;
6. stops dispatching before the budget is exhausted.

This provides cancellation, priorities, fairness, many instances of one function,
and a natural place to collect profiling data.

## Primitive 3: chunked loops

A long dynamic loop should compile to a resumable loop body:

```text
resume_loop(state, budget):
    while work remains and conservative_cost(next_iteration) <= budget:
        run iteration
        advance state
        subtract estimated cost
    if work remains:
        yield state
    else:
        finish result
```

The generated mcfunction does not need a literal `while`. It can recurse through a
bounded helper chain or unrolled block, then enqueue/schedule the continuation.

For branches and selectors with data-dependent cost, the static estimate must be
conservative or the runtime needs smaller checkpoints. Unbounded selector
cardinality requires a user bound, a runtime cap, or a diagnostic.

## Yielding cannot be invisible

Splitting an ordinary synchronous function across ticks changes semantics:

- other packs and game systems can observe intermediate state;
- entities and blocks can change between phases;
- the executor may disappear;
- return values are no longer immediately available;
- two tasks can interleave;
- reloads and server restarts can occur.

MDL should therefore expose multi-tick behavior in its types, for example:

```mdl
fn small_work() -> Int
task fn long_work() -> Future<Int>
```

or an equivalent effect such as `may_yield`. The compiler may freely split only
inside a region whose semantics permit yielding. Atomic/synchronous functions must
either fit the hard limit or fail compilation.

## Static diagnostics

The compiler should report bounds rather than merely “too large”:

```text
estimated command sequence: 420 + 7 * prisoners
declared prisoner bound: 128
worst case: 1,316 operations
soft budget: 1,000 operations/tick
hard server limit: 65,536 operations/sequence
decision: split after at most 82 prisoners per tick
```

For native entity forks it should similarly report maximum context products and the
assumption that supplied each cardinality.

## Remaining tests

- exact sequence boundary behavior at the default 65,536;
- deeper nested function-result and `execute` stage accounting;
- fork-zero command result, stored success/result, and `return run` behavior for a
  zero-output redirect;
- rejection of configured values above Java's signed-integer maximum;
- multiple `append` schedules of the same function;
- scheduled callback ordering within a tick;
- persistence across reload and server restart;
- queue fairness and cancellation;
- rehydrating players, entities, positions, rotations, and dimensions;
- safe behavior if a task's stored schema no longer matches after pack upgrade.

## Primary sources

- [Mojang's operation and fork accounting](https://feedback.minecraft.net/hc/en-us/articles/21968446892173-Minecraft-Java-Edition-1-20-3)
- [Current game-rule names and permitted ranges](https://feedback.minecraft.net/hc/en-us/articles/41809981427213-Minecraft-Java-Edition-1-21-11-Mounts-of-Mayhem)
- [Schedule `append`, `replace`, and `clear`](https://feedback.minecraft.net/hc/en-us/articles/360037384972-Minecraft-Java-Edition-Buzzy-Bees)
- [Minecraft Java 26.2 and data-pack format 107.1](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
