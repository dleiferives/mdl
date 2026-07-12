# mdl-compiler

`mdl-compiler` contains MDL's compiler data structures and passes. Its current Core
layer is a typed, ordered SSA control-flow graph with source provenance, verification,
derived analyses, deterministic printing, and guarded in-place editing.

Core intentionally contains no Minecraft commands or physical runtime representation.
Those enter in the target-aware IR and lowering stages.

The design and proof obligations are documented in
[`../../notes/compiler/stage-2-ssa-plan.md`](../../notes/compiler/stage-2-ssa-plan.md)
and
[`../../notes/compiler/stage-2-ssa-implementation.md`](../../notes/compiler/stage-2-ssa-implementation.md).
