# PS-15 — Advancement-Triggered Events

Status: **planned, researched to an implementation handoff (2026-07-22).** Depends on
[PS-13](ps-13-bot-driven-test-infrastructure.md) and [PS-14](ps-14-player-entity-kind.md)
(both complete). This document is written for a fresh agent with no memory of prior
sessions — read [`advancement-triggers.md`](../advancement-triggers.md) first for the
full researched Minecraft mechanics (advancement JSON shape, the one-shot re-fire
problem, the auto-revoke idiom); this document does not repeat that, it's the concrete
"how do I actually build this" pass, including one real correction to that note's own
syntax sketch.

## Why this exists, and why it's bigger than PS-13/PS-14

This is the push half of MDL's world-interaction story — every MDL program today runs
because something pulls it (a schedule, a call, `/function`); there is no event model
anywhere in the compiler. Unlike PS-14 (which generalized one already-generic enum
across a codebase written to expect a second variant), **this milestone adds a
genuinely new kind of top-level declaration to the language** — MDL has never had
anything like `on <trigger>(...) |binding| { ... }` before. Confirmed by reading
`frontend/ast.rs:14-20`: `AstModule` is `{ imports, structs, enums, functions, span }`
— a closed, fixed set of four top-level item kinds. This needs a fifth. Budget more
implementation time than PS-13/14 took; this touches the parser, a new AST node, new
HIR/Core/Minecraft-IR declaration tables, and a new emitted artifact kind, not just
existing generic machinery.

## The syntax decision — resolved here, do not re-litigate it

`advancement-triggers.md` §2.4 sketched `on inventory_changed(items: [...]) |player| {
...}` but deliberately left it undecided. Researched directly against this codebase's
existing grammar rather than inventing new forms:

```mdl
on inventory_changed(.items = ["minecraft:diamond"]) |player| {
    player.say("MDL_GOT_DIAMOND");
}
```

Three corrections/confirmations versus the original sketch, each grounded in a real,
already-existing MDL construct — **reuse, don't invent**:

1. **`|player| { ... }` is not new grammar at all.** MDL already has exactly this
   shape: `run.as(mc.entities(ArmorStand)...) |speaker| { ... }` (confirmed real usage
   at `frontend/parser.rs:2560`, and throughout `tests/source-fixtures/stage7/`) —
   a pipe-delimited single-name binding introducing a captured executor, usable
   inside the block with full entity-NBT-path-receiver capability (exactly the
   `reader.equipment...` shape PS-12 built). The event handler's `|player|` should
   be the *same* binding mechanism, not a new one — the checker work is "seed this
   function's initial context with `Player` already proven as current executor,"
   which is structurally the same proof `run.as(...)` produces, just supplied by
   the compiler instead of an explicit query, because vanilla itself guarantees the
   reward function runs `as`/`at` the triggering player. Go read how `run.as`'s
   executor-capture proof is built end to end (checker → HIR `HirContextStep` →
   ambient-context requirements) before designing this — it's the direct precedent.
2. **The original sketch's `items: [...]` argument syntax is wrong for this
   language and was corrected here, not carried forward.** MDL's actual, only named-
   argument form (S-027, `notes/syntax/named-arguments.md`) is a leading dot:
   `.parameter_name = expression`, not `name: expression`. The corrected form above
   uses `.items = [...]`. Verify this is still current before implementing (re-read
   `named-arguments.md`'s status line) — it was accurate as of this research pass.
3. **`ItemMatch` needs no wrapper syntax for this minimal slice.** `advancement-
   triggers.md` sketches `ItemMatch("minecraft:diamond")` as if it were a
   constructor call, but the Slice-1 `ItemMatch` is item-ID-only — a bare string
   literal in an ordinary S-031 bracket list literal (`.items = ["minecraft:diamond"]`)
   is sufficient and simpler, consistent with how the rest of this codebase avoids
   inventing wrapper syntax before it's needed (e.g. resource-id string literals
   already used verbatim in entity-NBT paths, `."minecraft:written_book_content"`).
   If `ItemMatch` grows predicates later (count, components), that's the point to
   revisit whether it needs constructor-call syntax — not now.

`on` is confirmed **not** a reserved keyword today (the complete closed keyword table
is in `frontend/lexer.rs:518-544`: `fn, pub, export, import, struct, enum, unsafe,
minecraft, run, const, var, if, else, while, switch, break, continue, return, true,
false, Bool, Int32, Void` — `on` isn't in it). Adding it is a new lexer keyword, plain
and uncontroversial.

**Write this up as its own file under `notes/syntax/` before or as the first step of
implementation** (e.g. `notes/syntax/event-handlers.md`, mirroring
`named-arguments.md`'s/`entity-paths-and-general-indexing.md`'s format: a status line,
the selected form, and the reasoning), per this project's own convention that syntax
decisions get a permanent, citable record separate from the milestone plan that
motivated them. This document's "Syntax decision" section above is the content for
that file; extracting it is close to copy-paste, not new research.

## The four architectural layers, in the order to build them

Unlike PS-14, there's no single "four sites, exhaustiveness-checker-guided" shortcut
here — each layer is genuinely new. Build and gate each before the next, the way
BE-1's own multi-stage plan did.

**A. Grammar: parser + AST.**
- `frontend/ast.rs`: new `AstModule.event_handlers: Vec<AstEventHandler>` field
  alongside `imports`/`structs`/`enums`/`functions` (line 14-20). New
  `AstEventHandler { trigger: AstName, arguments: Vec<AstNamedArgument>, binding:
  AstName, body: AstBlock, span: Span }` — check what the existing named-call-argument
  AST shape actually is (search for wherever S-027's `.name = expr` calls already
  parse to, likely something used by `AstCall`) and reuse that type for `arguments`
  rather than inventing a parallel one.
- `frontend/lexer.rs:518-544`: add `"on" => TokenKind::KeywordOn`.
- `frontend/parser.rs`: new top-level parse rule, modeled on however `run.as(...)
  |name| { ... }` parses today (find that parse function first — it already handles
  the identical `(...)  |name| { block }` tail shape) for the shared suffix, plus a
  new leading `on <identifier>(...)` header.
- Gate: a structural test that a source file containing an `on inventory_changed(...)
  |player| { }` declaration parses into the expected `AstEventHandler`, no checker/HIR
  involvement yet.

**B. Checker: HIR construction.**
- Recognize the trigger name (`EntityKind::from_source_name`-style closed lookup,
  `"inventory_changed" => Some(Criterion::InventoryChanged)`, anything else a
  diagnostic — mirror `BlockEntityKind::from_source_name`'s exact shape).
- Validate `.items = [...]` is present (required for `InventoryChanged` specifically —
  this validation is per-trigger, so structure it so a second trigger kind later
  doesn't require redesigning this, e.g. a small per-`Criterion`-variant argument
  contract, not one hardcoded check).
- Build the `|player|` binding's executor-capture proof — this is the piece to trace
  through `run.as`'s existing implementation most carefully, since seeding it
  *without* a source-level query expression (the compiler supplies `Player` as a
  given, not something the program queried for) may need a new HIR construction path
  even though the *proof shape* it produces should be identical to what `run.as`
  already produces.
- New `HirExternalDeclaration`-shaped (or sibling) node for the whole handler,
  analogous to how `HirFunction` represents an ordinary function today — find that
  type and mirror its shape for the parts that overlap (a body, a result contract of
  `Void` always, since reward functions have no return value).

**C. Core IR: `AdvancementDecl`/`Criterion`.**
- New `entity_id!`-declared `AdvancementId` (mirror `FunctionTagId`,
  `ir/minecraft/program.rs:12-15`).
- `AdvancementDecl { resource: AdvancementResourceId, criterion: Criterion, reward:
  McFunctionId, origin: OriginId }` — `reward` is `McFunctionId` directly, not the
  more general `InternalCallableRef` (`ir/minecraft/program.rs:23-28`) — a vanilla
  advancement's `rewards.function` is always exactly one function, `InternalCallableRef`
  additionally allowing `Tag` would model something vanilla's own JSON schema doesn't
  support.
- `Criterion` as a small closed enum, one variant for this slice:
  `InventoryChanged { items: Vec<ItemMatch> }`, `ItemMatch` as a thin wrapper around a
  validated item-resource-id string (mirror whatever this codebase's existing
  resource-id validation looks like — probably already exists for the
  `."minecraft:written_book_content"`-style resource-id schema keys, reuse it, don't
  rewrite it).
- The auto-revoke guarantee belongs here or in lowering, not in the checker: the
  generated reward function's command list gets `advancement revoke @s only
  <resource>` **prepended directly as the first command**, not via a separate wrapper
  function that calls into the body. Recommended, not yet proven: this project's own
  bias is against indirection that isn't earning its cost (see e.g. PS-12E deleting a
  once-necessary special case once it stopped being needed), and there's no evident
  reason a reward function needs a stack frame boundary between the revoke and the
  program-author's body. If implementation reveals a real reason for the wrapper
  (debugging, a lowering-order constraint), that's new information — take it over this
  recommendation.

**D. Minecraft-target lowering + emission.**
- `datapack/footprint.rs:9-16`: add `ArtifactFileKind::Advancement` (confirmed still
  exactly `{ Metadata, Function, FunctionTag }` as of this research pass — re-check,
  don't trust this document if other work has landed in between).
- `datapack/emit.rs`: new `serialize_advancement`, modeled directly on
  `serialize_metadata`/`serialize_tag` (`emit.rs:142-230` as of this reading) — same
  `#[derive(Serialize)] struct ... ` + `serde_json::to_vec` + trailing-newline pattern,
  not a hand-rolled JSON string. The vanilla shape (from `advancement-triggers.md`
  Part 1.1): `{"criteria": {"<name>": {"trigger": "minecraft:inventory_changed",
  "conditions": {"items": [{"items": [<item ids>]}]}}}, "requirements": [["<name>"]],
  "rewards": {"function": "<resource>"}}` — note **no `display` key at all** (omitting
  it, not setting it to some hidden/null value, is what makes it a fully hidden
  advancement — confirmed in the original research, re-verify the exact vanilla JSON
  key omission behavior is still what makes it hidden before shipping this, ideally
  against the real pinned server's actual accepted/loaded behavior, not just the
  wiki text `advancement-triggers.md` cites).

## Testing — this milestone's evidence story is structurally different from every other one

No Core-evaluator differential is possible or honest — `CoreEvaluator` has no player,
no advancement state, nothing to simulate. State this plainly in whatever plan you
write; don't manufacture a fake differential test to look complete. The real gate is
pinned-server-only, via PS-13's bot (`crates/mdl-test-bot/`, `cargo +nightly` from
inside that directory — see `ps-13-bot-driven-test-infrastructure.md` and PS-14's own
test for the working patterns, including the two-step book-giving syntax and the
tolerable `ShutdownTimeout`):

1. Install the compiled pack, connect the bot.
2. Satisfy the criterion — either drive the bot through a real container interaction
   (`open_container_and_click`, already proven working) or the simpler, more isolated
   `/give` directly to the bot if you want a narrower test that doesn't also depend on
   BE-1's container-read machinery working.
3. Assert the reward function ran (the established `say`-marker + `wait_for_command_log`
   pattern every other PS milestone's pinned tests already use).
4. **Repeat the same action and assert the reward fires a second time.** This is the
   entire proof the auto-revoke actually works, not just that an advancement fired
   once (every advancement does that by default, revoke or not, and a test that only
   checks the first fire would validate nothing beyond vanilla's own baseline
   behavior — this is not optional coverage, it is the milestone's actual claim).

Also worth a structural/differential test with no server at all: compile a source
file with an `on inventory_changed(...) |player| { ... }` declaration and inspect the
emitted `.mcfunction`/advancement-JSON text directly for the auto-revoke command and
the expected JSON shape — cheap, fast, and catches most JSON-shape mistakes before
ever touching the pinned server.

## Non-goals (inherited from `advancement-triggers.md`, unchanged)

- A general predicate/condition DSL, or multi-criterion AND/OR composition.
- Any trigger type beyond `minecraft:inventory_changed` — that's an explicit future
  milestone (Slice 2), grown as closed-table rows once this milestone's JSON-emission
  and auto-revoke machinery is proven reusable.
- Player-visible advancement trees — this stays a fully hidden event hook.
- Anything to do with reading NBT — that's BE-1/`block-entity-nbt-paths.md` entirely;
  composes at the source-program level (a reward function body can contain an
  ordinary entity-NBT or block-NBT read), never hardcoded here.
- `ItemMatch` predicates beyond item ID (count, components/custom data).

## Deferred, explicitly

- Whether the auto-revoke needs its own wrapper function or can be a direct prepend —
  recommended direct-prepend above, revisit only with a concrete reason.
- `ItemMatch` constructor-call syntax, if/when predicates grow beyond item ID.
- Trigger vocabulary growth beyond `InventoryChanged`.
