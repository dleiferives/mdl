# How Should the Compiler Select Runtime Representations?

## Core idea

MDL source types describe meaning. Backend representations describe how a value is
currently best realized in Minecraft. They must be separate.

```text
Typed value
  -> legal representation set
  -> usage/range/effect analysis
  -> selected or split physical representation
  -> commands
```

## Initial representation matrix

| Source value | Likely primary representation | Alternatives |
| --- | --- | --- |
| compile-time scalar | compiler constant | none at runtime |
| hot mutable `Int` | scoreboard score | NBT integer at boundaries |
| arithmetic real | scaled scoreboard integer | NBT float for transport |
| cold/serialized number | NBT numeric tag | macro literal |
| runtime `String` | NBT string | interned integer ID, pieces/tokens |
| displayed `Text` | structured text component | materialized NBT string only if required |
| `List<T>` | NBT list | unrolled tuple, scoreboard table, specialized layout |
| struct | NBT compound | scalarized/split fields |
| finite map | specialized dispatch | keyed NBT compound, entry list |
| entity reference | execution context/selector proof | UUID, tag, score ID |

## Conversion edges have costs

The compiler should model conversions explicitly:

```text
ScoreInt -> NbtInt
ScoreInt -> NbtFloat(scale)
NbtInt   -> ScoreInt
NbtValue -> MacroSyntax<T>
StringPieces -> NbtString
StringPieces -> Text
Aggregate -> ScalarFields
```

Repeatedly crossing an edge is a sign that representation selection was poor or a
conversion can be hoisted.

## Aggregate scalarization

Suppose a cell is stored as:

```snbt
{user_id:42, sentence_ticks:1200, display_name:"Alex", history:[...]}
```

If `sentence_ticks` is decremented every tick while the other fields are rarely
touched, keep the hot counter in a scoreboard and retain cold fields in NBT. Write
the counter back only when serializing or when a raw command can observe the compound.

This requires an explicit coherence rule around raw commands and externally visible
storage paths.

## Delay materialization

Keep these operations abstract in IR:

```text
StringConcat
ListBuild
AppendRange
MapLookup
ForEach
TextConcat
NumericCast
```

Lowering too early loses destination-sensitive choices. `StringConcat` might become
constant folding, a composite text component, one surrounding macro template, or a
materialized NBT string depending on its consumer.

## Optimization should search implementations, not only expressions

For a dynamic array read, legal implementations may include:

- macro-indexed NBT path;
- specialized leaf function;
- balanced score dispatch;
- scalar replacement;
- layout conversion;
- lookup elimination through a captured context.

These are whole implementation plans with setup, steady-state, and code-size costs.
A small e-graph may help explore pure expression identities inside a plan, but plan
selection also needs conventional data-flow analysis, dynamic programming, or a
dedicated equality/choice graph with effects and representation constraints.

## Profile-guided selection

Static estimates cannot know actual selector counts, index distributions, macro
cache locality, or list sizes. The compiler should eventually support profiles:

```text
call site -> invocation count
selector  -> observed cardinality distribution
macro     -> argument-tuple reuse/cache behavior
list op   -> size distribution
loop      -> trip-count distribution
```

It can then specialize hot IDs, pick dispatch thresholds, choose layouts, and decide
whether an auxiliary indexed representation pays for itself.

## Required compiler analyses

- constant propagation and partial evaluation;
- range/cardinality analysis;
- alias and escape analysis;
- aggregate scalar replacement;
- effect and observation analysis;
- loop analysis and vectorization/batching;
- representation liveness and conversion placement;
- function specialization and inlining;
- selector/context analysis;
- profile-guided optimization;
- source-to-command lowering explanations.

## Non-negotiable transparency

For every selected plan, `mdl explain` should show:

```text
selected: scoreboard-backed sentence_ticks
reason: 1,200 estimated arithmetic uses, 1 serialization use
inserted conversions: one ScoreInt -> NbtInt at save boundary
rejected: direct NBT update (3 commands per decrement)
```

That makes aggressive optimization understandable rather than magical.

