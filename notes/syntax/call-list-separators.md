# S-025 — Parameter and Argument Separators

**Status:** selected on 2026-07-13; amended by the uniform rule in S-035.

Function parameters and call arguments are separated by commas. Under S-035, a comma
after the final item is optional.

```mdl
fn execute(
    ops: Ops,
    mut machine: Machine,
) -> Void {
}

execute(
    ops,
    mut machine,
);
```

The same forms without final commas are equally valid, regardless of layout:

```mdl
fn execute(ops: Ops, mut machine: Machine) -> Void {}
execute(ops, mut machine);
execute(
    ops,
    mut machine
);
```

Newlines remain whitespace. They neither separate parameters/arguments nor determine
whether a final comma is legal. An empty list remains `()`.

Generic parameter lists, if added and comma-separated, inherit S-035.
