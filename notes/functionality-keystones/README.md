# Functionality Keystones

A *functionality keystone* is a real, independently useful program written against a target
platform MDL compiles to — something people actually built and shipped, not a hypothetical
capability list. The idea: instead of guessing which language/compiler features to build next,
find a real program that needs them, read how it actually works, and let that be the evidence.

This matters because guessing has a specific, repeatable failure mode this project has already
hit more than once elsewhere (see `../compiler/pre-scheduler/roadmap.md`'s own recurring
discipline of measuring against the real pinned server rather than trusting documentation): a
plausible-sounding feature description is not the same as what a real implementation actually
needs, and the gap between the two is often exactly where the interesting design work is. A
keystone forces that gap into the open before a milestone is scoped, not after.

## What lives here

Right now, one category: [`datapacks/`](datapacks/README.md) — real, published Minecraft
datapacks, downloaded and read directly to figure out which MDL systems are missing, partial, or
already sufficient to express the same program. See that folder's own README for the concrete
per-datapack convention.

Other categories may join later if a different kind of real target artifact turns out to be
useful evidence the same way (a real mod's data-driven behavior, a community-maintained function
library, etc.) — nothing about "functionality keystone" is specific to datapacks, that's just the
first and so-far-only source of them.

## What this is not

A keystone found here is scoping *evidence*, not a milestone by itself and not an implementation
plan. A gap surfaced in a keystone's notes still needs its own design pass (the way
`advancement-triggers.md` and `block-entity-nbt-paths.md` each preceded their own PS milestone)
before any code gets written against it.
