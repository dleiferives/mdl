# S-022 — Loop Control

**Status:** selected on 2026-07-13 for unlabeled loops.

`break;` exits the innermost enclosing loop:

```mdl
while (running) {
    if (finished) {
        break;
    }

    step();
}
```

`continue;` ends the current iteration of the innermost enclosing loop and proceeds
to its next condition check:

```mdl
while (running) {
    if (should_skip()) {
        continue;
    }

    process();
}
```

Both are simple statements and therefore require semicolons according to S-010.
Using either outside a loop is an error.

Switches do not fall through and do not consume `break`. Consequently, a `break;`
inside a switch arm exits an enclosing loop:

```mdl
while (running) {
    switch (op) {
        "stop" => break;
        else => process(op);
    }
}
```

The following remain separate decisions:

- labeled control of an outer loop;
- whether `break` may produce a value;
- interaction with future scheduled or suspendable bodies; and
- control transfer from compile-time loops or macros.
