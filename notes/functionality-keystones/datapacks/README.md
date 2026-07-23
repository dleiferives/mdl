# Functionality Keystones — Real Datapacks as Feature-Scoping Evidence

Each subdirectory here is one real, published Minecraft datapack, downloaded verbatim and
inspected directly (not summarized from its store page) to answer one question: **what would
MDL's compiler need to support to express this program?** This is the same evidentiary
discipline the compiler itself is held to elsewhere in `notes/` — prefer the real artifact over
documentation or a plausible-sounding assumption, the same principle
`ps-13-17-player-interaction-roadmap.md` states for the pinned server ("never assume a real
Minecraft NBT/command shape from documentation or precedent when the pinned server can be asked
directly"). Here the artifact is a real datapack instead of a real server, but the discipline is
identical.

This is a feature-scoping input, not a milestone queue by itself — a gap found here still needs
its own design note (mirroring `advancement-triggers.md`/`block-entity-nbt-paths.md`) before it
becomes a PS milestone. The value of this folder is turning "what should MDL support next" from
a guess into something grounded in what real, popular datapacks actually do.

## Convention

Each datapack gets `datapacks/<name>/`:

- The downloaded archive, as fetched, untouched (prefer an exact version/build that targets this
  project's own current Minecraft target version where one exists — check the source's version
  API rather than assuming its latest release matches).
- `extracted/` — the unzipped contents, so `.mcfunction`/`.json` files are directly greppable and
  readable in later sessions without re-downloading or re-extracting. If the pack ships
  version-range overlays (`pack.mcmeta`'s `overlays.entries`) or duplicate directory trees for an
  old/new pack-format split (e.g. plural `functions/`/`predicates/` vs. singular
  `function/`/`predicate/`), keep only whichever tree actually resolves for this project's current
  Minecraft target (check the overlay `min_format`/`max_format` ranges against that target's pack
  format) — the archive itself is the complete record if a future session ever needs the material
  for a different target version.
- `NOTES.md` — the actual deliverable. Not a description of the datapack's *feature list*
  (that's what its store page is for) — a technical account of **how it's actually implemented**
  (which vanilla mechanics/commands/NBT shapes it leans on) and, from that, **which MDL systems
  are missing, partial, or already sufficient** to express the same program. Cite exact files
  read, not paraphrases of a marketing page.

## Datapacks catalogued so far

| Datapack | Source | Notes |
|---|---|---|
| [veinminer](veinminer/NOTES.md) | [Modrinth](https://modrinth.com/datapack/veinminer), v1.3.5 (datapack loader, `26.2`) | Vein-mining via per-block-type mined-stat polling + relative-frame recursive flood-fill |
| [dynamic-lights](dynamic-lights/NOTES.md) | [Modrinth](https://modrinth.com/datapack/dynamic-lights), v1.9.3 (datapack loader, `26.2`) | Held-light-source tracking via self-rescheduling `schedule function` + marker-entity light handles + heavy predicate-tree use |
| [spawn-animations](spawn-animations/NOTES.md) | [Modrinth](https://modrinth.com/datapack/spawn-animations), v1.11.5 (datapack loader, `26.2`) | Dig-up spawn animation via `#minecraft:tick` + `schedule function`/`schedule clear`, per-tick work-budget selectors, and direct entity-`Pos`/`equipment` NBT writes |
| [brainfuck-interpreter](brainfuck-interpreter/NOTES.md) | [Modrinth](https://modrinth.com/datapack/brainfuck-interpreter), v0.5 (datapack loader, `1.21.11` only — no `26.2` build, weaker evidence, see its own note) | A real interpreter-in-a-datapack; direct comparison point for MDL's own PS-3 capstone and Stage 9's already-open static-command-bound question; one clean new gap (`minecraft:dialog`) |

## Why downloaded archives are tracked in git here, unlike server jars

`.gitignore` keeps large third-party binaries (the pinned server jar, `libraries/`, etc.) out of
the repo entirely, referenced instead from ignored local paths via env vars. Datapacks in this
folder are the opposite case on purpose: they're small (low tens of KB, not hundreds of MB), and
the entire point of this folder is being readable reference material for a future session with no
memory of this one — gitignoring the thing the notes are *about* would defeat that. If a future
datapack turns out to be unusually large, reconsider per-datapack rather than changing this
default silently.
