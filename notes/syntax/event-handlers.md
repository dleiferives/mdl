# S-043 — Advancement Event-Handler Declarations

**Status:** selected on 2026-07-22, implemented (grammar) the same day as part of PS-15.

A source file may declare a top-level push-model event handler:

```mdl
on inventory_changed(.items = ["minecraft:diamond"]) |player| {
    player.say("MDL_GOT_DIAMOND");
}
```

`on <trigger>(<arguments>) |binding| { <body> }` is a fifth top-level declaration kind,
alongside imports, structs, enums, and functions. `on` is a new reserved keyword.

## The selected form, and why each piece is what it is

- **`|player| { ... }` is not new grammar.** It is exactly the pipe-delimited single-name
  binding-capture shape `run.as(...) |speaker| { ... }` already uses to introduce a captured
  executor (`frontend/parser.rs`'s `parse_run_statement` capture parsing). The event handler's
  `|player|` reuses that identical token shape; what differs is entirely on the checker side
  (the compiler seeds `player`'s executor-capture proof itself, since vanilla guarantees the
  reward function runs as/at the triggering player — there is no source-level query to check).
- **`.items = [...]` is a leading-dot named argument**, the S-027-selected spelling
  (`.parameter_name = expression`). [`advancement-triggers.md`](../compiler/advancement-triggers.md)'s
  own sketch used `items: [...]`, which is not this language's named-argument syntax; that sketch
  is superseded by this decision.
- **The item list is a plain bracketed, comma-separated list of string literals**, not a
  wrapper constructor call (`ItemMatch("minecraft:diamond")` was sketched in the research note
  and rejected here as unnecessary indirection for an item-ID-only predicate).

## Correction to the research note's assumption: neither S-027 nor S-031 exist elsewhere yet

The design that motivated this decision assumed `.name = expr` (S-027) and `[a, b, c]` (S-031)
were already implemented elsewhere in the grammar and that this feature would simply reuse them.
Checking the actual parser/AST at implementation time found neither is implemented anywhere:

- S-027 (`notes/syntax/named-arguments.md`) is a *selected* spelling with no parser or AST code
  anywhere in the frontend — `AstCall.arguments` is a plain `Vec<AstExpression>`, positional only.
- S-031 (`notes/syntax/list-literals.md`) is likewise selected but unimplemented — `AstExpressionKind`
  has no list-literal variant, and `[` only ever appears in postfix index position
  (`pair[0]`, S-042), never as a value-constructing prefix.

Consequently, this feature does not "reuse S-027/S-031" in the sense of calling into existing
parsing code. It introduces new, deliberately narrow grammar scoped to exactly this one
position — an event handler's trigger-argument list — that happens to use the same surface
spelling S-027/S-031 already selected, so that if/when either is implemented generally for
ordinary calls and expressions, the spellings already agree and no source ever needs to change.
The new AST shape (`AstEventHandlerArgument { name, value }`, with
`AstEventHandlerArgumentValue::StringList(Vec<Span>)` as the only closed value kind so far) is
purpose-built for trigger arguments, not a general call-argument or list-literal facility, and
must not be treated as a precedent for either until they are separately decided and implemented.

## What this decision does not cover

- Which trigger names are recognized (`inventory_changed` only, Slice 1) and what argument
  contract each requires — a checker/semantic concern, not this grammar decision.
- `ItemMatch` predicates beyond a bare item-ID string (count, components) — the closed
  `StringList` value shape can grow a sibling variant later without redesigning this grammar.
- General named-argument or list-literal grammar for ordinary function calls — separate,
  unscoped future decisions (S-027/S-031 remain exactly as selected, still unimplemented outside
  this one use).

## Relationship to earlier decisions

Reuses S-013-style visual precedent (a leading-dot designator) established by S-027, and the
bracket-literal spelling established by S-031, without implementing either generally. Reuses the
executor-capture binding shape `run.as(...) |name| { }` already established (no syntax-note
citation exists for that shape specifically; it predates this decision ledger's finer-grained
entries and is documented in `notes/compiler/entity-nbt-path-composability.md` and PS-12's own
material). Does not touch S-042's postfix-indexing grammar.
