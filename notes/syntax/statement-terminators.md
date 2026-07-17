# S-010 — Statement Terminators

**Status:** selected on 2026-07-13.

Simple statements end with a semicolon. Newlines are whitespace and do not terminate
statements.

```mdl
const alpha: Int32 = 10;
var beta: Int32;

beta = get_beta();
step(mut machine);
return beta;
```

This rule applies equally when a simple statement is used as a switch arm:

```mdl
switch (op) {
    ">" => machine.pointer += 1;
    else => invalid_op(op);
}
```

A braced block is structurally terminated by its closing brace and does not require
a semicolon after that brace:

```mdl
if (ready) {
    run();
}

switch (op) {
    "[" => {
        begin_loop(mut machine);
        machine.depth += 1;
    }

    else => invalid_op(op);
}

fn run() {
    perform_work();
}
```

MDL does not use automatic semicolon insertion. Formatting may place a simple
statement across multiple lines, but its terminating `;` remains explicit.

S-012 separately requires a semicolon after a complete struct declaration. The
terminators used inside future constructs such as C-style `for` headers, other type
declarations, raw mcfunction blocks, and macro bodies will be decided with those
constructs rather than inferred from S-010.
