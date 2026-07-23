# Design and Research Notes

- [`compiler/`](compiler/README.md) — compiler architecture, pass proofs, semantic ambiguities,
  and the staged implementation roadmap (Stage 0 through the current pre-scheduler PS milestones).
- [`mcfunction/`](mcfunction/README.md) — Minecraft-command/backend research: Brigadier argument
  domains, NBT and macro mechanics, list/string operation lowering, and related target-side
  investigation.
- [`syntax/`](syntax/grammar.ebnf) — language syntax decision records (the `S-0xx` docs)
  and the authoritative grammar.
- [`functionality-keystones/`](functionality-keystones/README.md) — real, published programs
  (starting with Minecraft datapacks) read directly to ground which compiler features actually
  need to exist next, instead of guessing from a feature description.
