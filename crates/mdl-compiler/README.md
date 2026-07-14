# mdl-compiler

`mdl-compiler` is the in-memory library pipeline for MDL. It owns immutable source
text and provenance, bounded lexing and parsing, resolved typed HIR, verified Core
SSA, the closed baseline Core optimizer, structured Minecraft lowering and target
cost analysis, and deterministic datapack emission.

The public Stage 6 facade accepts one owned source unit and explicit typed options,
then returns every successful producer product without filesystem access. Ordinary
syntax and semantic failures retain their source context for pure diagnostic
rendering; downstream failures retain the earlier products specified by the facade
contract and their concrete typed producer error.

Core contains semantic computation and control flow, not rendered Minecraft command
strings or physical storage choices. Target-specific structure enters at the
verified lowering boundary. The complete architecture and proof checklist are in
[`../../notes/compiler/stage-6-minimal-frontend-plan.md`](../../notes/compiler/stage-6-minimal-frontend-plan.md)
and
[`../../notes/compiler/stage-6-todo.md`](../../notes/compiler/stage-6-todo.md).
