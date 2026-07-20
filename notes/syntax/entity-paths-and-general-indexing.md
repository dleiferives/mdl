# S-042 — Entity-NBT Path Member/Index Access and General Bracket Indexing

**Status:** proposed for PS-12 on 2026-07-20; not yet implemented.

The authoritative implementation design is
[`../compiler/entity-nbt-path-composability.md`](../compiler/entity-nbt-path-composability.md)
and [`../compiler/pre-scheduler/ps-12-entity-nbt-paths-plan.md`](../compiler/pre-scheduler/ps-12-entity-nbt-paths-plan.md).
This note records the syntax decision in isolation, the way S-032/S-033/S-041 do for their
features, so the surface spelling is reviewable independently of the backend work.

## What this replaces

Today exactly one Minecraft-entity value is readable from source, through exactly one hardcoded
method:

```mdl
run |reader| {
    const page: String = reader.main_hand_written_book_literal_page_or_empty(3);
}
```

`main_hand_written_book_literal_page_or_empty` is not a general capability — it is one method
name standing in for one fixed NBT path
(`equipment.mainhand.components."minecraft:written_book_content".pages[N].raw`), and no other
entity field is reachable at all. S-042 selects the general surface that this sugars away:

```mdl
run |reader| {
    const page: String = reader.equipment.mainhand.components."minecraft:written_book_content".pages[3].raw;
    const dynamic_page: String = reader.equipment.mainhand.components."minecraft:written_book_content".pages[index].raw;
}
```

with any prefix of the chain independently bindable and continuable:

```mdl
const item := reader.equipment.mainhand;
const content := item.components."minecraft:written_book_content";
const page: String = content.pages[index].raw;
```

## Two independent grammar changes

### 1. String-literal member key

`PostfixSuffix` gains a second `.`-introduced form, alongside the existing `.Name`:

```ebnf
PostfixSuffix = ".", Name
              | ".", StringLiteral
              | Arguments
              | "[", Expression, "]"
              ;
```

`.Name` is unchanged — it remains the spelling for every key that is a valid identifier
(`.equipment`, `.mainhand`, `.pages`, `.raw`). `.StringLiteral` is selected specifically because
some compiler-known keys are Minecraft resource identifiers (`minecraft:written_book_content`)
containing `:`, which is not a legal `Identifier` character and must not become one — resource
ids are a real, separately-typed domain elsewhere in the compiler
(`ResourceId<K>`/`FunctionResourceId`/`StorageId`, see
[`../mcfunction/native-value-carriers.md`](../mcfunction/native-value-carriers.md)'s 55-domain
inventory) and this syntax must not blur that distinction into ad hoc string concatenation.

`.StringLiteral` is accepted **only** where the checked receiver has a known schema entry whose
key equals the literal's exact content; there is no dynamic dictionary lookup and no
`Dict<String, V>`-style general map access hiding behind this syntax (see the *Deliberately not
selected* section).

Rejected alternative: exposing resource-id-keyed access through the same bracket form used for
indices, e.g. `.components["minecraft:written_book_content"]`. This was rejected because it
would make `[...]` ambiguous between "select a list element by position" and "select a compound
field by name" purely by the argument's static type, which is a harder disambiguation for a
reader than two distinct introducer tokens (`.` for a field-shaped step, `[` for an
element-shaped step) doing the same job `.Name` vs `[k]` already do for structs and tuples today.

### 2. General bracket index expression

```ebnf
PostfixSuffix = ...
              | "[", Expression, "]"    (* was "[", IntegerLiteral, "]" *)
              ;
```

This widens, rather than replaces, the PS-5 production (`../compiler/pre-scheduler/ps-5-anon-structs-destructuring-plan.md`).
`Expression` is a strict superset of `IntegerLiteral`, so `pair[0]` still parses identically to
before. What changes is that the bracket contents may now be *any* expression — a variable, a
call result, an arithmetic expression — which the type checker, not the grammar, accepts or
rejects depending on what kind of value is being indexed:

| Receiver kind | Accepted index form | Bounds rule |
| --- | --- | --- |
| `AnonymousStruct` (positional, PS-5/S-041) | compile-time integer literal only | checked against arity at compile time — **unchanged** |
| entity-NBT list schema node (this decision) | any `Int32` expression, constant or runtime | no source-level bounds concept — Minecraft's own missing-index behavior applies (fail-soft default, see the design note) |
| `List<T>` (S-032/S-033, decided, **not yet implemented**) | any `Int32` expression, constant or runtime, negative end-relative | checked at compile time when provable, else at runtime — a defined bounds failure, not silent |

This is a deliberate convergence, not a coincidence: S-041 already recorded the intent to "keep
the S-032/S-033 checked bracket surface consistent between static tuple indices and future
runtime list indices." S-042 is the point where the grammar actually becomes general enough for
that convergence to happen — **implementing S-042 for entity-NBT lists does not implement
S-032/S-033 for `List<T>`.** `List<T>` bracket indexing remains separately gated on its own
semantics (negative wraparound, the defined bounds-failure behavior S-033 requires, and how that
failure surfaces to a caller) — those are unimplemented today (`List<Int32>`'s only source
operations are `push`/`last_or_zero`/`without_last`; there is no source-level indexed read at
all yet). S-042 only commits to the grammar/AST shape being shared, so that whenever S-032/S-033
lands its own checker rule, it slots into the same production instead of requiring a third
bracket form.

## Disambiguation is by checked type, not by grammar

Exactly as `check_member_expression` already disambiguates a `.Name` suffix between nominal
struct field projection and PS-5 anonymous-struct named projection by the receiver's checked
`ValueType`/`SemanticType` — never by anything visible in the grammar — the checker disambiguates
`[Expression]` the same way: a positional-anonymous-struct receiver requires the index to reduce
to a compile-time literal (S-041's existing rule, verbatim); an entity-NBT-list-schema receiver
or (later) a `List<T>` receiver accepts a general expression. A program that writes
`pair[read_index()]` where `pair` is a positional anonymous struct is rejected with the same
diagnostic S-041 already specifies ("positional anonymous structs use compile-time indices"),
unchanged by this decision.

## What a chain means, source-level

A postfix chain of `.Name`, `.StringLiteral`, and `[Expression]` steps rooted at an
`Executor`-typed capture is not one atomic call — it is an ordinary left-associative postfix
expression, exactly like nominal struct field chains are today. Each intermediate step is a real,
independently typed, independently bindable value (`const item := reader.equipment.mainhand;`).
The final step's type is whatever the schema says it is (`String`, `Int32`, `Bool`, ...); nothing
about consuming, assigning, or passing that value differs from consuming an ordinary value of the
same type obtained any other way. There is no new "path value" or "reference value" kind exposed
by S-042 itself — a chain always terminates in an ordinary scalar read. (A distinct, later
decision — see [`../compiler/references-design.md`](../compiler/references-design.md) — covers
whether an *unterminated* chain, or an explicit reference-taking form, can be held and reused
across a mutation without repeating the whole walk; S-042 does not select that.)

## Deliberately not selected

- **General/dynamic compound-key access.** `.StringLiteral` is a closed-schema lookup, checked
  against a compile-time-known key set, not a `Dict<String, V>` operation. A truly runtime
  string used as a compound key remains out of scope everywhere in the language — this is a
  documented open safety problem
  ([`../mcfunction/nbt/dynamic-keys-and-path-safety.md`](../mcfunction/nbt/dynamic-keys-and-path-safety.md):
  rendering an arbitrary runtime string as one safe NBT-path segment has no safe general encoder
  in the compiler yet), and S-042 does not attempt to solve it.
- **Negative/end-relative indices for entity-NBT lists.** S-032's end-relative rule is a `List<T>`
  decision; S-042 does not extend it to entity-NBT list schema nodes, which use Minecraft's own
  positive-index-only list addressing.
- **A dedicated reference/pointer sigil.** No new token or prefix is introduced for "take a
  reference instead of reading a value." Chains remain ordinary value-producing expressions;
  reference semantics, if selected, are a separate decision layered on top, not a change to this
  grammar.

## Relationship to earlier decisions

Implements composability groundwork S-041 explicitly flagged ("future runtime list indices").
Reuses S-013/S-014's `.field` spelling precedent (there is already exactly one member-access
token, `.`, used for both nominal-struct and anonymous-struct projection; S-042 adds a second
introducer form after the same token rather than a new token). Leaves S-032/S-033 unimplemented
but grammar-compatible. Does not touch S-003 (`:=`), S-004 (`mut`), or any statement-level
grammar.
