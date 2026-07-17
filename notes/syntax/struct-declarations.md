# S-012 — Struct Declarations

**Status:** selected on 2026-07-13.

A plain product type is declared with `struct`, followed by its name and a braced
field list:

```mdl
struct Machine {
    pointer: Int32;
    instruction: Int32;
    tape: Tape;
};
```

This is a dedicated named-type declaration. `struct` is not currently an expression,
and the declaration does not model the type as a value stored in a `const` binding.
Any future system of compile-time type values must be designed separately.

## Fields and terminators

Fields use the same name-first type annotation as local bindings and parameters:

```text
field_name: FieldType;
```

Each field declaration ends with a semicolon. A semicolon after the closing brace
terminates the complete struct declaration:

```mdl
struct Empty {
};
```

Fields do not write `const` or `var`. For ordinary values, writability follows the
access path established by S-009:

```mdl
const fixed: Machine = create_machine();
fixed.pointer = 1; // error

var working: Machine = create_machine();
working.pointer = 1; // valid
```

Per-field immutability, if needed, is a separate future feature.

## Representation boundary

A plain struct defines named source fields and their types. It does not promise a C
ABI layout, declaration-order physical layout, one NBT compound representation, or
any other backend representation. The compiler may split, reorder, or independently
represent fields when observable source behavior is preserved.

The following remain separate decisions:

- struct-literal shorthand and update forms beyond S-013 through S-015;
- methods and other declarations inside a struct;
- visibility and attributes;
- structural versus nominal type compatibility; and
- explicitly laid-out or Minecraft-schema-bound struct forms.
