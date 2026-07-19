# Stage 8 Handoff

Status: **Stage 8 complete**

The compiler now has an executable scalar physical-realization boundary. Semantic
Core still contains immutable typed values and calls; Minecraft lowering separately
owns facts, score/frame storage, realization occurrences, exact uses,
materializations, ABI modes, and activation lifetime.

## Guarantees inherited by the next stage

- A semantic scalar may have zero, one, or several verified physical occurrences.
- Score storage reuse is justified by exact sparse half-open live segments.
- Synchronous selector multiplicity does not force dynamic storage when child
  activations are proven serial.
- Direct and mutual scalar recursion use frames only on reachable recursive SCC
  edges; acyclic code retains static score homes.
- Recursive fields are typed from the frozen physical declaration (`Bool`→NBT byte,
  `Int32`→NBT int), not re-derived during emission.
- Recursive result destinations are proven disjoint from caller spill destinations.
- Every selected physical recipe is counted before resource allocation and every
  emitted command is structured, verified, length-checked, traced, and reconciled.
- Unused recursive runtime support is absent.
- External users can inspect activation kind, depth ownership, command-limit
  assumptions, exported score ABI, reports, target maps, and artifact traces.

## Runtime protocol

For a recursive edge the caller appends `{}`, stores exact live/clobbered scores into
typed tail fields, writes direct-score parameters, invokes the callee, restores
spills, copies demanded pinned results, removes the tail frame, and continues.
Normal source `return` cannot bypass caller cleanup.

The protocol is synchronous. Exceeding Minecraft's command-sequence limit can leave
world effects, score state, and private frame residue. The generated load function
is the explicit recovery entry and resets the frame list. It cannot roll back world
effects.

## Evidence entry points

- Implementation checklist: [`stage-8-todo.md`](stage-8-todo.md)
- Completion audit: [`stage-8-completion-audit.md`](stage-8-completion-audit.md)
- Realization model: [`stage-8/8-a-realization-model.md`](stage-8/8-a-realization-model.md)
- Recursive ABI: [`stage-8/8-d-recursive-frames.md`](stage-8/8-d-recursive-frames.md)
- Test workflow: [`testing-harness.md`](testing-harness.md)
- Semantic/failure contract: [`semantic-ambiguities.md`](semantic-ambiguities.md)

## Next boundaries

Stage 8.5 is the immediate client of this handoff. Its
[`pre-scheduler roadmap`](pre-scheduler/roadmap.md) first strengthens semantic and
vanilla test oracles, then applies the realization boundary to aggregates, owned
lists, strings, and the synchronous capabilities needed by an ordinary MDL
Brainfuck interpreter. The aggregate client plan is
[`aggregate-values-plan.md`](aggregate-values-plan.md); its first fixed-layout slice
is now assigned to PS-2.

Stage 9 adds persistent scheduling after that validation. It must not reuse
synchronous tail frames across ticks, and it must preserve the source loop/fuel/value
semantics already proven during Stage 8.5.
