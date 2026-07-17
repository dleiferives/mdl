# S-009 — Read-Only Access Through `const`

**Status:** selected on 2026-07-13 for ordinary values.

Every place reached through a `const` binding is read-only. This prevents both
rebinding the root and mutating one of its fields or elements:

```mdl
const machine: Machine = create_machine();

machine = other;      // error: machine is const
machine.pointer += 1; // error: pointer is reached through machine
machine.tape[0] = 10; // error: the element is reached through machine
```

The rule follows the access path used by the source expression. It does not create a
distinct `const Machine` type and does not declare that every `Machine` value is
globally immutable.

A value read through a `const` binding may initialize a writable binding:

```mdl
const original: Machine = create_machine();
var working: Machine = original;

working.pointer += 1; // valid through the var binding
```

The exact copy and alias semantics of this initialization remain a separate
decision. S-009 establishes only that `working` is a writable access path and
`original` is not.

## Calls and derived places

A place derived through a `const` binding cannot be passed to a `mut` parameter:

```mdl
const machine: Machine = create_machine();

step(mut machine);         // error
increment(mut machine.pc); // error
```

Read-only calls and ordinary reads remain valid:

```mdl
const machine: Machine = create_machine();

const pc: Int32 = machine.pc;
inspect(machine);
```

If first-class references are added later, forming a reference through a `const`
access path must not provide a way to recover writable access. The exact reference
syntax and permission model are not selected yet.

## External handles remain open

This decision covers ordinary source values. It does not decide whether a constant
binding to an entity, block, storage location, score, or other external Minecraft
handle prevents mutation of the referenced world state. Handle identity and target
permissions must be designed together with the source place model.
