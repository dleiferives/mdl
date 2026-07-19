# How Should Branches and Basic Blocks Be Lowered?

## Recommendation

Build an ordinary typed SSA control-flow graph first. Do not decide that every basic
block is an mcfunction. After optimization, lay the CFG out using a mixture of:

- fused straight-line blocks;
- inline guarded commands;
- small return-based dispatch helpers;
- outlined cold or shared branch bodies;
- duplicated or outlined joins;
- linear, tree, or macro switch dispatch;
- materialized Boolean scores only when their stability or reuse justifies them.

The default safe `if/else` lowering should use a generated dispatcher that evaluates
the condition once. The fastest lowering for a particular branch can be selected
after effect, cost, and profile analysis.

## Correctness trap: evaluating the condition twice

This apparently natural translation is not generally correct:

```mcfunction
execute if data storage mdl:branch {state:{x:0}} run function mdl:branch/then
execute unless data storage mdl:branch {state:{x:0}} run function mdl:branch/else
```

The 26.2 test made the then-function change `x` from 0 to 1. The second command then
observed the new value and ran the else-function too:

```text
#then = 1
#else = 1
```

Two guarded commands are valid only when the compiler proves that the condition is
stable between them. Raw commands and unknown calls are observation/effect barriers.

## Safe default: return-based dispatcher

Caller:

```mcfunction
function mdl:generated/if_7/dispatch
# common continuation remains here
```

Dispatcher:

```mcfunction
execute if data storage mdl:branch {state:{x:0}} run return run function mdl:generated/if_7/then
return run function mdl:generated/if_7/else
```

This was verified on vanilla 26.2:

- the condition was evaluated once;
- exactly one branch ran when the then-branch mutated the tested state;
- `return` exited the generated dispatcher only;
- the caller continued through its common continuation.

If an arm contains one command, it can be placed directly after `return run`:

```mcfunction
execute if score #condition mdl.tmp matches 1 run return run scoreboard players add #result mdl.reg 1
return run scoreboard players remove #result mdl.reg 1
```

Multi-command arms require functions or another legal fused layout.

## Safe alternative: snapshot the Boolean

Materialize the condition once:

```mcfunction
execute store success score #condition mdl.tmp if data storage mdl:state {ready:true}
execute if score #condition mdl.tmp matches 1 run function mdl:generated/then
execute if score #condition mdl.tmp matches 0 run function mdl:generated/else
```

This also ran exactly one mutating branch in the 26.2 test.

Snapshotting is attractive when:

- the Boolean is reused;
- control rejoins in the current function;
- both arms need the value;
- outlining a dispatcher would create more calls;
- the condition is effectful and must execute once.

It costs a temporary score and an additional command/stage, so it should not be the
default for every condition.

## Measured hard-limit comparison

Six equivalent Boolean branch loops were run with
`max_command_sequence_length=1000`. Each iteration updated a loop counter, selected
between `sink += 1` and `sink -= 1`, and recursed. Results were identical for true and
false conditions.

| Lowering | Iterations before limit | Approx. sequence operations/iteration | General safety |
| --- | ---: | ---: | --- |
| Inline `if` + `unless`, inline arms | 167 | 6 | Only if condition remains stable |
| Snapshot score + two inline guards | 125 | 8 | Safe |
| Direct return dispatcher, inline arms | 167 | 6 | Safe |
| Return dispatcher + arm functions | 143 | 7 | Safe |
| `if` + `unless` calling arm functions | 143 | 7 | Only if condition remains stable |
| Branchless scoreboard arithmetic | 143 | 7 | Safe if arithmetic/overflow semantics match |

The loop overhead is included in every row, so the table is a comparative result,
not a universal isolated cost for one branch.

Preliminary `stopwatch` runs over 20,000 iterations broadly favored the inline and
direct-dispatch forms after warm-up, but individual results varied substantially due
to JVM compilation and runtime noise. We should not encode wall-time weights from
this small test. A repeatable multi-process benchmark with warm-up, randomized order,
many samples, and confidence intervals remains necessary.

## Branch lowering decision table

### Compile-time condition

Delete the unreachable arm. No runtime branch.

### `if` with no `else`

For one command:

```mcfunction
execute if <condition> run <command>
```

For a larger body:

```mcfunction
execute if <condition> run function <body>
```

There is no second evaluation, so mutation inside the body is not a correctness
problem.

### Small `if/else`, stable condition

Use two guarded commands or guarded functions. This can match the best measured
sequence efficiency. Stability requires that the then-arm cannot change anything
read by the condition.

### Small `if/else`, potentially unstable condition

Use a direct return dispatcher. Inline one-command arms after `return run`.

### Large arms with a common continuation

Use a dispatcher with outlined arm functions, then return to the caller's common
continuation. Consider inlining the hot arm and outlining the cold arm.

### Condition reused multiple times

Materialize a normalized 0/1 score once and reuse it.

### Value-producing branch

Keep an SSA phi/block argument in IR. During lowering, either:

- coalesce both arm results into one scoreboard/storage destination;
- have a predicate function return the selected integer;
- retain the value as a deferred condition if the only consumer is another branch.

Do not materialize a Boolean merely because the source language has a `Bool` value.

## Boolean representation

A typed Boolean should be able to remain in one of several backend forms:

```text
ConstBool
DeferredCondition
NormalizedScoreBool       // proven 0 or 1
CommandSuccess
```

`CommandSuccess` is not silently interchangeable with a language Boolean. Command
failure, no execution context, result value zero, and `return fail` have distinct
Minecraft behavior and need typed conversions.

A deferred condition tree can contain atoms such as:

```text
ScoreMatch
ScoreCompare
DataExists/Matches
EntityExists
BlockMatches
ItemMatches
PredicateResource
FunctionPredicate
```

Keeping conditions deferred enables fusion, reordering, and avoidance of temporary
scores.

## AND, NOT, and OR

### AND

Minecraft naturally short-circuits a chain of execute conditions:

```mcfunction
execute if score #a mdl.tmp matches 1 if score #b mdl.tmp matches 1 run <command>
```

For pure atoms, order cheap/selective tests before expensive NBT or selector tests.
Reordering is illegal when reads have side effects or context dependencies.

### NOT

Use `unless` or invert a normalized score condition. Push negations down through a
pure Boolean tree during optimization.

### OR

Two independently guarded copies can execute the body twice when both inputs are
true. A short-circuit predicate helper is safe:

```mcfunction
execute if score #a mdl.tmp matches 1 run return 1
execute if score #b mdl.tmp matches 1 run return 1
return 0
```

Both of these consumers were verified on 26.2:

```mcfunction
execute if function mdl:predicate/or run <command>
execute store result score #value mdl.tmp run function mdl:predicate/or
```

Complex pure Boolean expressions can be simplified with e-graphs, but selecting an
efficient decision tree/short-circuit order is a separate optimization problem. A
BDD-like representation or costed decision DAG may eventually help share repeated
tests.

## Switch and pattern matching

### MDL implementation split

Decision recorded 2026-07-19: implement the typed language and baseline target path
before adding cost-directed switch selection.

The first source slice should provide closed enums and exhaustive Zig-style
`switch` expressions/statements. Integer switch prongs should support exact values,
inclusive closed ranges, multiple patterns with one body, and `else`; enum prongs
should support inferred enum literals such as `.running`. The checker owns duplicate,
overlap, unreachable-prong, type, result-join, and exhaustiveness diagnostics. These
are language semantics and must not depend on a Minecraft optimization profile.

For this first slice, generated datapack size is not a selection constraint. The
baseline may emit many helper functions, hardcoded cases, and repeated static command
forms when that keeps the implementation direct and the runtime behavior credible.
Continue recording function, line, and byte counts, but do not reject or compact a
correct program merely because those counts are large.

HIR/Core should retain the fact that a test is an exact value or integer range long
enough for Minecraft lowering to emit the existing typed
`Condition::ScoreMatches(ScoreRef, ScoreRange)` primitive. The initial backend may
use one deterministic, source-ordered test chain with the existing safe branch
lowering and a default target. Correct code generation is the gate; it does not need
to choose an optimal dispatch shape yet.

The implementation scope deliberately stops at syntax, name/type/flow checking,
target-independent evaluation, and one mechanical target legalization. Do not add
case reordering, range coalescing, balanced trees, macro dispatch, profile-guided
layout, or range-analysis-driven specialization to the first slice. Emitting native
`execute if score ... matches ...` for one source range is legalization of that
range, not a dispatch optimization.

TODO after the baseline is measured: add a semantics-preserving, target-owned
dispatch chooser. It should compare coalesced range tests, source/hotness-ordered
linear dispatch, balanced range trees, and validated macro dispatch. Do not encode a
magic threshold such as `case_count < 100` in source semantics. If an early tuning
knob is useful, make a target/optimization option such as
`max_linear_switch_cases`, with a deterministic default and the fallback always
available. The eventual cost model must account for executed command steps, function
calls, emitted lines/bytes, case density, shared destinations, known value ranges,
profile information, and Minecraft macro cache behavior.

TODO after representative programs exist: benchmark runtime work and wall-clock
behavior across switch widths, densities, value distributions, shared destinations,
and repeated invocations. Record generated function/line/byte counts alongside those
measurements, then decide whether and where datapack size should enter `speed`,
`balanced`, and `size` optimization profiles. Until that evidence exists, prefer the
simple static lowering even when it produces a large pack.

### Small or probability-skewed switch

Use a linear early-return dispatcher, ordering hot cases first:

```mcfunction
execute if score #kind mdl.tmp matches 0 run return run function mdl:case/0
execute if score #kind mdl.tmp matches 1 run return run function mdl:case/1
return run function mdl:case/default
```

This was verified for matching and default cases on 26.2.

### Larger bounded integer switch

Generate a balanced range-dispatch tree. Static pre-parsed branches cost roughly
logarithmic comparisons instead of a linear scan.

### Dense or repeatedly dispatched switch

A macro can place an integer/enum into a function resource ID:

```mcfunction
$function mdl:case/$(kind)
```

This offers direct dispatch but incurs macro argument bridging, instantiation, and
possible cache misses. It also requires exhaustive range validation. Compare it
against the static tree using the target profile.

### String switch

Prefer compiling a finite string enum to an integer discriminant. General runtime
strings otherwise require macro dispatch, NBT comparison, a trie, or interning.

### Merge equivalent cases

Cases with the same body should become score ranges or shared targets. Common case
prefixes/suffixes should be factored like ordinary CFG regions.

## Basic blocks should not map one-to-one to functions

One-function-per-block is a useful initial emitter because `function` and `return
run function` provide jumps. It is not the desired optimized layout.

### Block fusion

Merge a block with its successor when:

- the predecessor has one successor;
- the successor has one predecessor;
- no yield/schedule boundary intervenes;
- command/context constraints permit it;
- code-size policy permits duplication/fusion.

This removes a function invocation and exposes command fusion.

### Hot-trace fallthrough

Arrange the likely path as straight-line commands and outline the unlikely path:

```mcfunction
execute unless <hot-condition> run return run function mdl:cold_trace
# hot body inline
# hot continuation inline
```

The cold trace can call an outlined join or duplicate a small join. This minimizes
hot-path calls at the expense of code size.

### Shared join function

Both arms tail-call a shared join. This minimizes pack size but adds a function
invocation on every path.

### Duplicate a small join

Copy a short common continuation into both traces. This removes a hot function call
but grows the pack. Use a size-versus-runtime threshold.

### Dispatcher returns to caller join

The verified dispatcher layout lets both arms return to a caller whose following
commands are the join. It avoids duplicating or calling the join, at the cost of one
dispatcher function invocation.

### Tail calls

Use `return run function` for a CFG edge when the current generated function has no
work after the successor. It expresses the edge without retaining a logical return
continuation, but the function invocation still consumes command budget.

## Suggested CFG pipeline

1. Build typed SSA CFG with block arguments/phi nodes.
2. Constant-fold and remove unreachable arms.
3. Simplify pure Boolean expressions.
4. Track each condition's read set and each arm's write/effect set.
5. Prove whether dual evaluation is stable.
6. Estimate branch probabilities from constants, hints, or profiles.
7. Form hot traces and fuse straight-line blocks.
8. Choose inline, snapshot, dispatcher, tree, macro, or branchless lowering.
9. Place/duplicate/outline joins using a cost model.
10. Coalesce phi destinations and allocate scoreboard/storage temporaries.
11. Factor shared `execute as/at/in` context prefixes.
12. Verify sequence and fork bounds for every emitted root.

## Minecraft-specific composition optimizations

- Fuse repeated context prefixes into one outlined function.
- Fuse compatible conditions into one `execute` chain.
- Push cheap pure failures before expensive tests.
- Convert per-entity branch loops into direct native bulk commands when possible.
- Hoist loop-invariant conditions out of repeated bodies.
- Specialize functions for constant branch arguments.
- Inline small hot helpers; outline large cold helpers.
- Eliminate repeated dynamic lookups before branching on their result.
- Preserve entity cardinality so a Boolean existence test is not confused with
  context iteration.
- Treat raw commands as effect barriers unless explicitly annotated.

## Static multi-tick tasks

A yielding task branch should usually select a statically generated next phase:

```text
if condition:
    task.pc = THEN_PHASE
else:
    task.pc = ELSE_PHASE
```

The fixed task lane resumes next tick and dispatches on `pc`. Alternatively, the
branch schedules one of two precompiled continuation functions. No dynamic job queue
is required, but context/live locals must still be preserved across the tick.

## Remaining measurements

- robust wall-time benchmark distributions for every lowering;
- one-command versus multi-command arm sizes;
- scoreboard, NBT, entity, block, item, and function predicates;
- true/false and skewed branch distributions;
- helper inlining thresholds;
- join duplication thresholds and pack reload cost;
- linear versus balanced versus macro switches;
- condition ordering and selector fusion;
- per-entity dispatch under different cardinalities;
- branch behavior across scheduled static task phases.

## Primary sources

- [Mojang's `return run`, function result, and `execute if function` semantics](https://feedback.minecraft.net/hc/en-us/articles/21968446892173-Minecraft-Java-Edition-1-20-3)
- [Mojang's execute operation accounting](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-3)
