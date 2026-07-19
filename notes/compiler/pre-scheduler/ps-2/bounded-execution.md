# PS-2 Bounded Synchronous Execution

Status: **runtime-fuel rehearsal implemented; static deployment proof is deferred**

## Goal

Make pre-scheduler dynamic programs terminate according to an explicit language
contract. The PS-3-ready slice implements ordinary returned fuel state and keeps
Minecraft's existing conservative `NoFiniteBoundProven` result honest; it does not
yet infer a static command bound from runtime fuel.

## Three distinct limits

Do not collapse:

1. **Program/input bound** — maximum text or normalized instructions accepted.
2. **Semantic fuel** — maximum Brainfuck instructions dispatched.
3. **Minecraft hard limits** — target command-sequence and per-redirect fork rules.

Fuel exhaustion is normal typed program behavior. A Minecraft hard-limit abort is an
abnormal deployment failure that may leave world and compiler-private residue.

## Fuel semantics

Recommended contract:

- fuel is a nonnegative bounded integer supplied or selected at the public entry;
- each attempted dispatch of a normalized Brainfuck instruction consumes exactly
  one unit;
- bracket validation/parsing consumes its own bounded parse work, not interpreter
  fuel;
- completion on the final available unit succeeds;
- zero fuel with a nonempty unfinished program returns `StepLimitExceeded` without
  dispatching; and
- the result states what output/partial tape/program position remain observable.

Fuel should be visible in ordinary MDL code so PS-3 does not depend on a special
compiler watchdog.

## Deferred static work composition

A future target-analysis extension should combine:

```text
maximum parse units
  * conservative cost per parse unit
+ maximum dispatched instructions
  * conservative cost per dispatch
+ setup/finalization/call/frame costs
```

Operation cost may depend on list representation, bracket search, macro cache state,
or input sizes. Preserve finite bounds, capped bounds, no-finite-bound-proven, and
unknown. Never substitute a guessed average for legality.

If bracket matching scans the program dynamically, its per-dispatch cost also
depends on maximum program length. A preprocessing pass that constructs jump
relationships has a different bounded setup/steady-state tradeoff. Both are library
implementation plans whose target costs must be compared.

## Deferred deployment policies

The exact public configuration syntax remains provisional, but the compiler needs
separate choices such as:

```text
strict: require ProvenWithin hard limits
guarded: emit with stated runtime/precondition contract
unchecked: compile while reporting unknown/unbounded compatibility
```

The current compiler emits its nonfatal conservative report. A future strict mode
must reject rather than silently raising world gamerules.

Soft per-tick budgets do not cause automatic splitting until Stage 9. A program may
fit the hard sequence limit yet be too slow for a healthy tick; report that concern
without claiming a scheduler exists.

## Cleanup and recovery

Normal semantic fuel exhaustion must unwind ordinary source calls and leave
compiler-private synchronous frames balanced. It is not implemented by deliberately
hitting Minecraft's command-sequence limit.

Exact hard-limit probes separately retain the Stage 8 contract: interruption may
leave partial world effects and frame residue, and the load/recovery entry resets
compiler frames without rolling back world state.

## PS-3-ready evidence

- fuel zero, exact completion, one short, and surplus;
- ignored input units and invalid brackets independent from dispatch fuel;
- the same returned state under all four optimization-policy products;
- Core evaluator step, call-depth, and value-node caps; and
- a pinned vanilla composition differential without gamerule mutation.

## Evidence deferred with static enforcement

- parse-limit versus fuel-limit precedence;
- nested Brainfuck loops with a finite fuel cap;
- cost bound changes under None/Baseline without semantic changes;
- strict acceptance/rejection at exact Java 26.2 sequence/fork boundaries;
- normal fuel cleanup versus abnormal command-limit residue;
- readable explanations connecting source bounds to target work summaries.

## Stage 9 handoff

Stage 9 may transform finite or open-ended work into persistent phases. It must
preserve the same semantic fuel/result contract while adding suspension points,
persistent state, and context reconstruction. Stage 8.5 must not encode fuel state
in a layout that the scheduler is forced to treat as source identity.
