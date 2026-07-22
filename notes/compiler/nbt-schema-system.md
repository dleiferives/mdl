# The Compiler-Known Entity/Item/NBT Schema System

Date: 2026-07-20
Status: **design — not yet implemented. `entity-nbt-path-composability.md` §2.4 sketches one
instance of this table (6 rows); this note specifies the general mechanism the table is an
instance of, so growing it is "add a row," not "redesign the table."**

## 1. What problem this solves

`entity-nbt-path-composability.md` needs a way to answer, at compile time: *given a receiver
already narrowed to some schema node, what are its known children, and what does each one
narrow to?* That question has to be answered the same way whether the node is `EquipmentSlots`,
`ItemStack`, `Components`, or (later) something not yet imagined — block entity data, a different
item component, a different Minecraft version's shape for the same logical field. Without a
general mechanism, every new field is a new hand-written checker branch, which is exactly the
per-intrinsic sprawl this whole redesign exists to stop (`macro-reference-crossing-model.md`'s
"there is no macro command" principle, applied one layer up: *there should be no hardcoded
schema branch*).

This note specifies **one data-driven table format** that the checker walks generically, and the
registration discipline for adding a row, so "the schema grows" never means "the checker's code
changes."

## 2. The core data shape

A schema is a graph of **nodes**. Every node is one of four kinds:

```text
SchemaNode = Scalar(ValueType)
           | Compound { fields: BTreeMap<SchemaKey, SchemaNode> }
           | List { element: Box<SchemaNode> }
           | Optional { inner: Box<SchemaNode> }     // see §5 — deliberately NOT used by default
```

- **`Scalar(ValueType)`** is a terminal — `String`, `Int32`, `Bool` today, matching MDL's existing
  closed `ValueType` set. Reaching a `Scalar` node ends the chain; the chain's type is that
  `ValueType` (or `DataRef<that type>` under the `ref` binding form, `references-design.md` §3).
- **`Compound { fields }`** is a node reachable by `.name` or `."string literal"` steps
  (`entity-paths-and-general-indexing.md` §1). Each field is itself a `SchemaKey -> SchemaNode`
  entry — the *only* legal way to walk further.
- **`List { element }`** is a node reachable by `[Expression]` steps
  (`entity-paths-and-general-indexing.md` §2), narrowing to `*element` regardless of whether the
  index is `Const` or `Runtime` — the schema does not care which; that distinction belongs to the
  Core-level `Operand<i32>` on the emitted `NbtPathSegment::Index`, not to typing.
- **`Optional { inner }`** exists in the data model for completeness but is not used by the first
  schema instance — see §5 for why "missing" is handled by the fail-soft default contract
  instead, and when `Optional` would actually get used.

```text
SchemaKey = Identifier(String)     // reachable via ".name"   — must be a legal MDL Name
          | ResourceId(String)     // reachable via ".\"string\"" — must NOT be a legal MDL Name
                                    //   (this is enforced at registration time, see §4.3)
```

## 3. Roots — where a chain is allowed to start

A schema chain does not start from nothing; it starts from a **root**, which is a typed,
already-checked value the language already produces. The first (and, for PS-12, only) root:

```text
Root::CurrentExecutor { kind: EntityKind }
```

produced by an `ExecutorCapture` (`run |reader| { ... }`), exactly as today — `reader`'s checked
`SemanticType::Executor(ExecutorType)` is the thing a chain's first `.equipment` step narrows
from. A root is registered against the **top-level schema table for its kind** — i.e., "what does
`.equipment` mean starting from an `Executor`" is itself one row in a root-keyed table, not a
special case in the checker:

```text
RootSchemaTable: EntityKind -> Compound { fields: ... }
```

Future roots (§8 of `references-design.md`'s target kinds) — a captured `EntityRef`, a
block-entity handle, a command-storage location — each register their own entry in an analogous
root table, reusing the same `SchemaNode` walking logic unchanged. See
[`block-entity-nbt-paths.md`](block-entity-nbt-paths.md) for the block-entity case worked out in
detail — it confirms this claim rather than complicating it, modulo one real addition
(match-indexed lists, for container slots).

## 4. Registration — how a new field actually gets added

This is the part that makes "composable, not hardcoded" a checkable claim rather than a slogan.
Adding a new known field is exactly:

### 4.1 One data declaration

```text
// illustrative, not a proposed Rust API — the point is the shape, not the syntax
schema_field(
    parent: ItemStack,
    key: SchemaKey::Identifier("count"),
    node: SchemaNode::Scalar(ValueType::Int32),
)
```

No new `MinecraftSemanticKey`. No new `MinecraftRecipeId`. No new `SelectedSemanticRecipe`
variant. No new HIR expression kind. No new checker function. This is the concrete difference
from the system being retired (`entity-nbt-path-composability.md` Part 1.1) — there, one new field
meant a new descriptor, a new recipe, a new contract projection, and a new arm in every exhaustive
match those types participate in. Here, one new field means one table row.

### 4.2 The path from a table row to an emitted command is entirely mechanical

Once a row exists, everything downstream is generic machinery that already has to exist for the
*first* field to work, so it costs nothing extra per additional field:

1. The unified chained-postfix checker (`entity-nbt-path-composability.md` §2.3) looks up the
   step against the current node's `fields` map — generic `BTreeMap` lookup, not per-field code.
2. On success, the chain's accumulated `NbtPath` gains one more `NbtPathSegment::Key`
   (`Operand::Const`, always — see §7) or `NbtPathSegment::Index` (`Operand<i32>`, const or
   runtime).
3. On reaching a `Scalar` node, HIR construction (`HirExpressionKind::EntityNbtPath`,
   `entity-nbt-path-composability.md` §2.5) uses that node's `ValueType` as `result_ty` —
   generic, not field-specific.
4. Lowering (§2.6 of the same note) and the macro/reference engine (`macro-system-specification.md`)
   consume the finished `NbtPath` with zero awareness of which schema table produced it.

### 4.3 Registration-time validation (a table row can be malformed; the compiler must catch it)

- An `Identifier` key must be a legal MDL `Name` — this is what makes it reachable via `.name`
  instead of requiring `."string"` (`entity-paths-and-general-indexing.md` §1's exact
  disambiguation rule, restated as a schema-table invariant: **a key is registered as
  `Identifier` if and only if it is spellable as `.name`; otherwise it must be registered as
  `ResourceId`.** A key that is a legal identifier but registered as `ResourceId` anyway, or vice
  versa, is a schema-authoring bug the registration step should reject, not a checker special
  case.
- No two fields of the same `Compound` may share a key (ordinary map-uniqueness, but stated
  because `entity-nbt-path-composability.md`'s worked table has to satisfy it and any future
  table must too).
- A cycle in the node graph is rejected at registration time — the schema is required to be a
  DAG (in practice a tree, since two different parents having the same child node is unlikely to
  arise honestly from Minecraft's own NBT shapes, but not itself forbidden).

## 5. Missing/absent data — why `Optional` is not the default answer

`written-books-26.2.md`'s frozen Phase-1 contract is explicit: a broken link anywhere in the chain
(missing equipment, wrong item, absent page, wrong component shape) yields a **type-appropriate
default**, silently, not a language-level `None`/optional value the caller must unwrap. This is a
deliberate, already-frozen decision this schema system inherits rather than revisits:

- The schema's job is to say *what a well-formed value at this path looks like*, assuming the
  chain resolves. It is not the schema's job to model "what if it doesn't" — that is a uniform
  cross-cutting lowering behavior (init-then-attempt-read, `entity-nbt-path-composability.md` §2.6),
  applied identically regardless of which schema node is the target, not a per-node fact.
- `SchemaNode::Optional` therefore has **no user in the first schema instance**. It exists in the
  data model (§2) for a different, later situation: a field whose *absence itself is meaningful
  and distinguishable* from a type-appropriate default (e.g., a `Bool` field that is either
  present-and-true, present-and-false, or genuinely absent, where collapsing "absent" into
  `false` would lose real information a program might need). No such field is in scope for PS-12;
  §2 names the node kind now so a future schema author doesn't have to invent it under time
  pressure, and so this note doesn't have to be revised to add it later.

## 6. Versioning — why this table, not the closed-verb recipe system, owns version-dependence

`written-books-26.2.md` records real version sensitivity: *"The older `HandItems[0]` path also
fails... this is why target-version recipe selection owns the path."* Under the retired system,
"the path" was owned by a `MinecraftRecipeId` selected per `(target, key)`. Under this system,
**the schema table itself is keyed by target version**, not the path-construction mechanism:

```text
SchemaTable: JavaEditionTarget -> RootSchemaTable
```

A version where `equipment.mainhand` doesn't exist (pre-1.20.5-shaped hand items, per the note's
own history) registers a **different** `RootSchemaTable` for that target — same mechanism, same
`SchemaNode` shapes, different data. This is strictly more scalable than the retired approach:
one new Minecraft version's item-component reshuffle is a new schema table (potentially sharing
most nodes with the previous version's table, differing only where Mojang actually changed
something), not a new recipe for every affected intrinsic. The *lowering* mechanism
(`entity-nbt-path-composability.md` §2.6) never needs to know a version changed anything; it only
ever sees "the schema said this key exists, here is its `NbtPath` segment" — version-awareness is
fully contained in which table got selected before checking started, exactly mirroring how
`MinecraftRecipeId::for_semantic_key(target, key)` used to be the one place target-awareness lived
for the old system, except now scoped to *table selection* instead of *per-operation recipe
selection*.

## 7. Why compound keys are always `Operand::Const`, never `Operand::Runtime`

Restated and justified here (already stated as a rule in `macro-system-specification.md` §6 rule
5): every `SchemaKey` a chain step resolves against is known at **schema-authoring time**, and
every chain step's key is checked against the schema at **compile time** — there is no path by
which a `SchemaKey` lookup ever depends on a runtime value. `NbtPathSegment::Key` therefore never
needs an `Operand<...>` wrapper the way `NbtPathSegment::Index` does; this is a structural
consequence of the schema design (closed, compile-time-known key sets), not an arbitrary
restriction bolted on afterward. This is also exactly why the harder unsolved problem
(`dynamic-keys-and-path-safety.md`'s "rendering an arbitrary source string as exactly one safe
NBT-path segment") never has to be solved for this feature to ship: the schema system sidesteps
the general problem entirely by never accepting a runtime string as a key in the first place.

## 8. Worked instance — the full node graph this note's mechanism must support first

Restated here in full node-graph form (not just the summary table
`entity-nbt-path-composability.md` §2.4 gives), because "must support first" deserves to be exact:

```text
RootSchemaTable[JavaEditionTarget::V26_2][EntityKind::ArmorStand] =
  Compound {
    "equipment" (Identifier) -> Compound {
      "mainhand" (Identifier) -> ItemStack,
      "offhand"  (Identifier) -> ItemStack,
      "head"     (Identifier) -> ItemStack,
      "chest"    (Identifier) -> ItemStack,
      "legs"     (Identifier) -> ItemStack,
      "feet"     (Identifier) -> ItemStack,
    }
  }

ItemStack = Compound {
  "id"         (Identifier) -> Scalar(String),
  "count"      (Identifier) -> Scalar(Int32),
  "components" (Identifier) -> Components,
}

Components = Compound {
  "minecraft:written_book_content" (ResourceId) -> WrittenBookContent,
}

WrittenBookContent = Compound {
  "pages"    (Identifier) -> List { element: BookPageEntry },
  "author"   (Identifier) -> Scalar(String),
  "title"    (Identifier) -> BookPageEntry,
  "resolved" (Identifier) -> Scalar(Bool),
}

BookPageEntry = Compound {
  "raw" (Identifier) -> Scalar(String),
}
```

`ItemStack.id`/`.count` are included even though the book-page chain doesn't use them, precisely
because PS-12's exit criteria (`ps-12-entity-nbt-paths-plan.md`) require proving extensibility by
adding a genuinely new field as part of the audit — `.count` is that field, pre-specified here so
the worked example in the plan has a concrete target already designed, not invented ad hoc during
the audit itself.

## 9. What this note deliberately does not specify

- **The concrete Rust representation** of `SchemaNode`/`SchemaKey`/`RootSchemaTable` (const data?
  a build-time-generated table? a `phf` map?) — an implementation choice for PS-12B, not a design
  commitment this note needs to make.
- **Write support.** This note (matching `entity-nbt-path-composability.md`'s explicit non-goal)
  specifies read-shaped schema resolution only. A schema-typed *write* target (needed for
  `ref var` write-through, `references-design.md` §3.2) reuses the identical node-graph/lookup
  mechanism — nothing here is read-only by construction — but the write-specific lowering
  concerns (does writing a `Compound` node require full-subtree replacement vs. per-field patch,
  per `native-compound-algebra.md`'s "merge is not a general patch language" finding) are not
  worked out here and should not be assumed solved.
- **Block-entity or command-storage schema tables** — the mechanism (§2–§7) is written generally
  enough to host them, but no such table is specified; that is explicit follow-up work per
  `references-design.md` §11.

## 10. What "done" looks like

1. `SchemaNode`/`SchemaKey`/root registration exist as real types, with §4.3's validation rules
   enforced (not merely documented) at registration time.
2. The checker's chained-postfix resolution (`entity-nbt-path-composability.md` §2.3) is generic
   over the table — it contains no field name, resource id, or entity-kind literal anywhere in its
   own code; every one of those lives only in the table data from §8.
3. §8's full node graph is the registered table for `V26_2`/`ArmorStand`, and the book-page chain
   type-checks by walking it generically end to end.
4. The PS-12 exit audit's "add `.count`" step (§8's pre-specified proof) requires touching only
   the table data, confirmed by diffing what changed — zero checker/HIR/lowering code in that
   diff.
5. §6's per-target-version table selection is real (even if only one version's table is populated
   at first) — i.e., the table is keyed by `JavaEditionTarget` from day one, not retrofitted once
   a second version is needed.
