# PS-4 enums, switch, and ranges

This ordinary MDL program is the independent PS-4 semantic fixture. It exercises
nominal enum construction, inferred and qualified variants, enum values in a
struct and private calls, expression and statement switches, exact/multiple/
inclusive integer patterns, signed boundaries, and an exported scalar-only ABI.

`cases.json` is the hand-written expected-result table used by evaluator and
vanilla-server tests. The program deliberately includes `0...20`, which must reach
Minecraft as the inclusive score range `matches 0..20`.
