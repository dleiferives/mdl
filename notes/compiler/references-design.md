# References — Full Design (Value-Crossing Model Tier B)

Date: 2026-07-20
Status: **design — not yet implemented; no code exists for this note's content**

This note is the full treatment of the reference half of the value-crossing duality first
stated in [`macro-reference-crossing-model.md`](macro-reference-crossing-model.md) §5. That
note sketches references in a few paragraphs as the mirror image of macros. This note exists
because a few paragraphs is not enough to build or review against — it works through identity,
invalidation, aliasing, mutation, lifetime, source syntax, and the exact IR shape, target kind
by target kind, so that "I want my refs" has a concrete, checkable answer rather than a
one-line gesture at Tier B.

It is written to stand alone: a reader should be able to implement against this note without
needing to reconstruct intent from source code, which — as of this writing — implements none
of it.

## 1. What a reference is, precisely

A **reference** is a compiler-tracked capability to read and/or write one specific NBT location
(or scoreboard cell) *without copying its value out first*, together with enough compile-time
information to know when that capability has gone stale. It is the source-language expression of
the "static NBT path → indirection → runtime value" side of the crossing duality:

```text
Reference:  static NBT path   ──indirection──→   runtime value
Macro:      runtime value      ──$(var) at a .mcfunction boundary──→   static command text
```

A reference is **not**:

- a general pointer with pointer arithmetic — there is no address space to walk, only a typed
  path from a known root;
- an alias that survives the target's structural change — see §4, invalidation is a first-class
  concept, not an afterthought;
- a runtime value at all in the MDL type sense — it never occupies a scoreboard cell or a plain
  NBT leaf; it is compiled away, either into direct command syntax (an indirection bridge) or
  into a macro-carried path fragment (§7), never into a "reference object" living in the
  generated datapack.

This mirrors `DataRef<T>` as sketched in
[`../mcfunction/nbt/references-identity-and-ownership.md`](../mcfunction/nbt/references-identity-and-ownership.md):

```text
DataRef<T> = {
  target_kind,       // Storage | Entity | Block | Score
  target_identity,   // StorageId / selector-derived entity proof / block position / score holder
  validated_path,     // NbtPath, built from typed schema steps (§6) and/or checked list indices
  type,                // T — the pointee's MDL type
  lifetime/invalidation facts,  // §4
}
```

## 2. Why references exist as a distinct concept from ordinary values

MDL's frozen default is deep-copy aggregate semantics (PS-2 decision, restated in
[`representation-selection.md`](../mcfunction/representation-selection.md)): assigning or passing
a struct, list, or NBT-shaped value copies it. This default is correct for source clarity — most
code should not think about aliasing — but it is wrong for exactly the cases references exist to
handle:

- **Repeated access to the same nested location.** `reader.equipment.mainhand.components."minecraft:written_book_content".pages[i].raw`
  evaluated three times in a row (read, compare, write) should not re-walk and re-copy the whole
  chain three times if the source intent is "the same page, examined and then updated."
- **In-place mutation of one field deep inside a larger structure**, without copying the whole
  structure out, mutating, and copying back (`native-compound-algebra.md`'s measured behavior:
  `data modify` already supports targeted field writes natively — the compiler should expose
  that, not hide it behind full-structure round-trips).
- **Passing "the same slot" into a function** that both reads and writes it, without the caller
  manually re-deriving the path after the call (this is `DataRef<T>`'s answer to a subset of what
  S-004 `mut` parameters were deferred to solve — see §9).

`dynamic-access.md` names this directly: *"If code is already executing as the relevant player
or cell entity, capture the current context once and carry it through the IR. Re-running a
dynamic lookup for each field is worse than preserving the proven reference."* References are
that capture, made a first-class, typed, checkable part of the language rather than an internal
compiler optimization the source program cannot ask for or reason about.

## 3. Source syntax

### 3.1 Taking a reference

An explicit keyword, not sigil punctuation (matching MDL's existing preference for explicit
keywords over terse sigils — `const`/`var`, not `let`/`let mut`; `delete`, not a bare postfix
operator):

```mdl
ref const page = reader.equipment.mainhand.components."minecraft:written_book_content".pages[index];
```

`ref const` binds an **immutable reference** — read-only capability to the location, still typed
as `DataRef<BookPageEntry>` (§6's schema type for that node), not as `BookPageEntry` itself.
`ref var` binds a **mutable reference**:

```mdl
ref var page = reader.equipment.mainhand.components."minecraft:written_book_content".pages[index];
page.raw = "new text";     // writes through the reference, in place
```

This is deliberately the *same* postfix chain S-042 selects for ordinary value access — `ref`
does not introduce new path syntax, it changes what the chain *produces* (a `DataRef<T>` capability
instead of an immediate value read). This is important for composability: the schema table
(§6, shared with `entity-nbt-path-composability.md` §2.4) is walked identically either way; only
the terminal step differs (materialize now vs. capture the path).

### 3.2 Reading and writing through a reference

```mdl
const raw_text: String = page.raw;      // read-through: ordinary field syntax on a DataRef
page.raw = "updated";                    // write-through: ordinary assignment syntax
```

A `DataRef<T>` where `T` is itself schema-shaped (a compound node, not a terminal scalar) supports
the *same* postfix chaining as the underlying schema — `page.raw` on a `DataRef<BookPageEntry>`
narrows to a `DataRef<String>` internally before the terminal read, exactly mirroring how
`reader.equipment.mainhand` narrows the schema walk in the non-`ref` case. A reference to a
compound node is not automatically materialized into a value; only a scalar-typed reference, used
in a value position, triggers the read-through.

### 3.3 Passing a reference to a function

```mdl
fn set_page(ref var target: BookPageEntry, text: String) {
    target.raw = text;
}
...
ref var page = reader.equipment.mainhand.components."minecraft:written_book_content".pages[index];
set_page(page, "hello");
```

A `ref` parameter mode mirrors the binding-site spelling. Passing a `DataRef<T>` copies the
capability (the path + identity + invalidation facts), never the pointee — this is the one place
MDL's copy-default is deliberately not "the whole value," matching the type's whole purpose.

## 4. Invalidation — the part that makes this safe, not just convenient

A reference is a *proof*, valid only as long as its target's shape and location haven't changed
underneath it. This section is the compiler-owned contract for when that proof still holds,
adapted directly from the measured/researched rules in
[`../mcfunction/nbt/references-identity-and-ownership.md`](../mcfunction/nbt/references-identity-and-ownership.md).

| Target kind | Survives | Invalidated by |
| --- | --- | --- |
| compound field (`.name` step) | unrelated sibling field insert/remove/write | that field or an ancestor compound being removed or replaced wholesale (e.g. `set value {...}` on an ancestor) |
| list index (`[i]` step) | unrelated element writes at other indices | insertion, removal, sort, or reversal of the containing list — the same numeric index may now name a *different* element, silently |
| entity-rooted path | the entity remaining loaded and alive | the entity unloading (temporarily unresolvable — see below) or being killed (permanently invalid) |
| block-rooted path (future; §11) | — | dimension/chunk-unload/block-entity-type change, block replacement |
| command-storage-rooted path | the compiler's own storage layout | nothing else — this is the cleanest target kind because MDL owns the whole namespace, *except* a raw command or another datapack mutating a path MDL also uses (see §5) |

The compiler tracks, per live reference, the **narrowest invalidating operation set** reachable
from the reference's declaration to each of its uses (a live-range analysis, exactly like the
value-crossing live ranges in the macro model — see §8's shared machinery). Three enforcement
levels, in order of preference:

1. **Statically proven safe.** No invalidating operation exists on any path between the
   reference's creation and a given use (e.g., a `ref var page = ...; page.raw = "x";` with no
   intervening list mutation). No runtime check is emitted — the reference lowers straight to
   the static NBT path.
2. **Statically proven invalid.** A definite invalidating operation exists on every path to a use
   (e.g., `ref var page = list[0]; list = other_list; page.raw = "x";` — `page` unconditionally
   refers to a list that was just replaced). This is a **compile-time error**, not a warning —
   references do not degrade to silently reading garbage.
3. **Not statically decidable.** The reference may or may not still be valid depending on a
   runtime condition (e.g., invalidation inside one branch of an `if`, not the other). This
   requires an explicit language-level failure story — see §4.1. This case must never be silently
   treated as case 1.

### 4.1 What "not statically decidable" does at runtime

This is the one genuinely open question this note does not resolve, because it is a real language
design fork, not an implementation detail:

- **Option A — reject at compile time.** Treat "not statically decidable" the same as "statically
  proven invalid": a reference used across any control-flow join where an invalidating operation
  occurred on *any* incoming edge is a compile error, full stop. Simpler to implement and reason
  about; more programs rejected than strictly necessary (conservative).
- **Option B — re-validate at the use site.** Emit a runtime check (e.g., `execute if data ...`
  testing the path still exists / still matches the expected shape) before a read/write through a
  possibly-stale reference, with a defined failure behavior (matching S-033's "defined bounds
  failure, not silent" precedent) when the check fails.

This note's recommendation, to be confirmed as its own decision before implementation: **start
with Option A.** It is strictly simpler, it cannot silently produce wrong results, and it matches
how S-033 chose "defined failure" over "guess" for list bounds — the same instinct applied one
level earlier (reject rather than guess) is consistent and lower-risk to ship first. Option B
remains available as a later loosening once real programs demonstrate the conservative rejection
is a practical problem, exactly the pattern PS-4/PS-5 used (ship the simple mechanical rule,
measure, generalize later).

## 5. Effect and alias analysis — what the compiler must track

A reference is only sound if the compiler can answer, at every program point: *"has anything I
don't control possibly invalidated this reference?"* This requires:

- **Alias analysis** — do two references (or a reference and an ordinary access) name overlapping
  paths? Two references into the same list at statically-different indices never alias; two
  references into the same list at possibly-equal indices might.
- **Effect analysis** — does an intervening operation (assignment, list mutation, function call)
  touch the reference's target or an ancestor of it? A function call is conservatively assumed to
  invalidate any reference passed into it (or reachable through a captured executor/entity),
  unless the callee's signature proves otherwise (see §9's `mut`/`ref` parameter interaction).
- **Raw-command observation barriers.** Per `stage-6-effects-plan.md`'s existing rule ("Raw
  commands block context fusion, command duplication, condition re-evaluation, and
  effect-sensitive motion across the barrier") and `references-identity-and-ownership.md`'s
  explicit warning ("a command-storage reference is the cleanest compiler-owned reference, but
  raw commands or other packs can still mutate a public path"): **any** `unsafe minecraft(...)`
  statement is treated as a full invalidation barrier for every live reference, unconditionally,
  regardless of whether the raw command textually mentions the reference's storage path. This is
  conservative by necessity — the compiler cannot prove what a raw command does.

This is the same category of analysis the value-crossing model already requires for
crossing-placement optimization (hoisting, batching — see
[`macro-reference-crossing-model.md`](macro-reference-crossing-model.md) §2, §8), and is expected
to share implementation machinery with it, not duplicate it: both are asking "is this live range
still valid at this program point," just for two different kinds of liveness (a runtime value's
crossing-readiness vs. a path's addressing-validity).

## 6. Typing: references compose with the schema system, they don't duplicate it

A reference's pointee type is exactly a node in the compiler-known schema table specified in
[`nbt-schema-system.md`](nbt-schema-system.md) — the same table
[`entity-nbt-path-composability.md`](entity-nbt-path-composability.md) walks for ordinary
(non-`ref`) reads. `DataRef<T>` is generic over *any* schema-typed `T`, including:

- a terminal scalar node (`DataRef<String>`, `DataRef<Int32>`, `DataRef<Bool>`) — the common case,
  read/write-through to a leaf;
- a compound schema node (`DataRef<ItemStack>`, `DataRef<BookPageEntry>`) — supports further
  chaining (§3.2) before any value materializes;
- a list schema node (`DataRef<List<BookPageEntry>>`) — supports `[index]` chaining, itself
  producing a `DataRef<BookPageEntry>`.

There is exactly one path-walking mechanism in the compiler (the schema-typed postfix chain);
`ref` only changes what happens at the point a chain would otherwise terminate in a value read.
This is the same "one mechanism, two encodings" discipline the macro/reference duality already
commits to at the IR level (§7) — it is not an accident that it shows up again here at the type
level.

## 7. Lowering: references share the crossing engine's bridge vocabulary

References do not get their own command-emission machinery. Reads and writes through a reference
lower to exactly the **Ref-encoding indirection bridges** the value-crossing model already names
(`macro-reference-crossing-model.md` §1, §4 step 3): `... from storage`, `execute store result
... run scoreboard players get ...`, `scoreboard players operation`. These are the same bridge
forms the macro engine's BRIDGE step already emits for operand crossings — a reference read is a
Ref crossing at the point of materialization, nothing new.

**A dynamic (runtime-valued) step inside a reference's path is a macro crossing on the path
itself**, restated from `macro-reference-crossing-model.md` §5.1: `NbtPathSegment::Index(Operand::Runtime(v))`
already represents exactly this, and already lowers through the generic `crossings.rs` engine
(once its two confirmed bugs — see [`entity-nbt-path-composability.md`](entity-nbt-path-composability.md)
Part 1.2–1.3 — are fixed). A reference with a runtime list index is therefore not a new
lowering case: it is an ordinary schema-typed path (§6) whose materialization point is deferred
(§3) rather than immediate. The **only** genuinely new lowering concern references add beyond
what the crossing engine already does is invalidation-check emission (§4.1, if Option B is ever
selected) and reference-argument passing convention (§9) — everything else is reuse.

### 7.1 Physical representation of a live reference

A reference is **not materialized as a runtime value** in the general case. Where the whole path
is compile-time-known (every step is a `Const` segment), the "reference" is purely a compiler-side
fact — a `NbtPath` + target identity carried in the compiler's own bookkeeping, with zero runtime
footprint until a read/write through it actually emits a command. Where the path contains a
runtime index, the reference still is not a first-class runtime value: what's runtime is the
*index value* (an ordinary `Int32` living in its own home, exactly as today), and the reference
remains a compile-time pairing of "which home holds the dynamic index" with "the rest of the
static path shape." This is why references cost nothing extra at rest — the cost is paid only at
each read/write-through, exactly once per occurrence, which is the entire performance point of
having them (§2).

## 8. Placement: references have live ranges too

Restated from `macro-reference-crossing-model.md` §2: a `Ref` crossing has a live range and a
placement, exactly like a `Subst` crossing does. The optimizations that fall out are the
reference-side mirror of the macro placement table:

| Reference placement move | Effect |
| --- | --- |
| Hoist a repeated read through the same reference to before a loop | one indirection bridge instead of N |
| Coalesce a read immediately followed by a write-back of the same field | skip the redundant read if the write fully overwrites |
| Sink a write to the last point before the reference goes out of scope / gets invalidated | fewer intermediate writes observed by nothing |
| Eliminate a reference entirely when every use is proven within one straight-line region with no aliasing risk | inline back to direct static-path commands, zero reference bookkeeping at all |

This table is deliberately not fleshed into an algorithm in this note — it belongs to the same
future crossing-placement optimization pass `macro-reference-crossing-model.md` §2 defers, and
should be designed once, covering both encodings, not twice.

## 9. Relationship to `mut` parameters (S-004)

S-004 selected `mut` parameter syntax and deferred its semantics, explicitly recording (in
[`ps-5-anon-structs-destructuring-plan.md`](pre-scheduler/ps-5-anon-structs-destructuring-plan.md)):
*"a `mut` parameter is copy-in/copy-out sugar that lowers to an ordinary parameter plus an
additional result written back at the callsite."* That is a **different** mechanism from `ref`:
`mut` copies in, mutates a local copy, copies out at return — no aliasing exists during the call,
so two `mut` references to overlapping state are (per that note) probably rejected outright.
`ref` is true in-place aliasing — no copy ever happens, the callee genuinely writes through to the
caller's storage location, and two `ref var` parameters that alias are a real runtime hazard
(§5's alias analysis exists specifically to catch this).

These serve different intents and should both exist: `mut` for "give me a fresh mutated copy back
without me writing awkward reassignment," `ref` for "let me operate on the actual location without
copying a potentially large structure at all." Implementing `ref` does not retire the `mut` design
question; if anything it sharpens it — `mut`'s eventual overlapping-place verdict (S-026's open
item) can reuse `ref`'s alias analysis (§5) as its enforcement mechanism once both exist, rather
than inventing a second aliasing story.

## 10. What `ref const` proves that ordinary read access doesn't

It is fair to ask why `ref const page = ...; const text = page.raw;` is better than just
`const text = reader...pages[index].raw;` directly — for a single read, it isn't; both compile to
the same one indirection bridge. The value is exactly the repeated-access case (§2): `ref const`
lets a chain be walked **once** and the resulting narrow-typed handle reused for several
subsequent reads (`page.raw`, and later, once the schema grows, `page.some_other_field`) without
re-walking the shared prefix each time — a source-visible common-subexpression elimination the
programmer opts into explicitly, rather than hoping the optimizer finds it. `ref var` adds the
in-place-write capability on top.

## 11. Target kinds beyond entities

This note and `entity-nbt-path-composability.md` both scope their first concrete client to
entity-rooted paths (the written-book chain). The `DataRef<T>` model above is written generally
enough (target_kind: `Storage | Entity | Block | Score`) to extend to:

- **Command-storage-rooted references** — the compiler's own heap; cleanest case (§4's table),
  natural next client once entity paths ship, since command storage is where compiler-managed
  aggregates (lists, structs) already live.
- **Block-entity-rooted references** — same shape as entity, different invalidation facts
  (dimension/chunk-load/block-entity-type, per §4's table); no schema table exists for block NBT
  yet and would need its own `nbt-schema-system.md`-style registration work before this is usable.
- **Score-rooted references** — a `DataRef<Int32>` whose target is a scoreboard cell rather than
  an NBT path; read/write-through still lowers to a Ref-encoding bridge (`scoreboard players get`/
  `scoreboard players operation`) instead of a `data` command, but the source-level model is
  unchanged.

None of these are selected as in-scope for the first implementation (PS-12 stays entity-only);
this section exists so the general design isn't accidentally entity-shaped where it doesn't need
to be.

## 12. Open questions carried from the source research

Restated from `references-identity-and-ownership.md`, not yet answered by this note and not
blocking a first entity-only implementation, but real questions before target kinds beyond
entities (§11) or general-purpose reference-heavy code is expected to work well:

- creation/deletion policy for any future keyed-record style usage (a UUID-keyed external
  "table," per the `EntityMap` research) — out of scope until MDL has any such construct;
- stale-record cleanup and UUID-reuse assumptions for entity-rooted references specifically —
  affects §4's entity row once references are expected to outlive a single function's execution
  (persisted across ticks is a Stage 9 scheduling concern, not answered here);
- whether a reference itself can be stored inside an aggregate (a struct field of type
  `DataRef<T>`) — not selected either way; the syntax in §3 only covers local bindings and
  parameters.

## Summary — what would need to be true for this to be "done"

Matching the acceptance-criteria style of `macro-reference-crossing-model.md` §7:

1. `ref const`/`ref var` bind a `DataRef<T>` from the same schema-typed postfix chain S-042
   selects for ordinary reads, with no separate path grammar.
2. Read-through and write-through use ordinary field/assignment syntax on the bound reference —
   no special reference-dereference operator.
3. Every reference has a compiler-tracked live range and invalidation fact set; a statically
   provable invalidation is a compile error, never a silent stale read (§4, §4.1's Option A).
4. Reference reads/writes lower through the same Ref-encoding bridge vocabulary the crossing
   engine already owns — no parallel reference-specific command-emission path exists.
5. A reference costs nothing at rest; cost is paid once per read/write-through, and repeated
   access through one reference is provably cheaper than repeating the chain (§10's whole point).
6. `mut` (S-004) and `ref` remain two distinct, both-real mechanisms with a recorded relationship
   (§9), not one subsuming the other by accident.
