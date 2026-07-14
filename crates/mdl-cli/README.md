# MDL CLI

`mdl` is the Phase 1 single-file driver for the in-memory `mdl-compiler`
pipeline. It compiles one UTF-8 source file, reports every generated callable ABI,
and safely materializes the complete datapack only after compilation succeeds.

```console
cargo run -p mdl -- compile program.mdl \
  --output build/my-pack \
  --core-opt baseline \
  --minecraft-opt baseline \
  --namespace my_pack \
  --register-objective my.pack \
  --description "My generated pack"
```

Every CLI-exposed choice is explicit. The only supported target is pinned to
Minecraft Java Edition 26.2; the remaining Phase 1 policies are fixed rather than
additional flags:

- at most 1,000,000 tokens including EOF, also used as the whole-compilation Core
  join-edge-operand expansion cap;
- syntax depth 256 (the hard maximum), 100 ordinary frontend diagnostics, and at
  most one final truncation finding;
- target-default command-sequence and fork assumptions of 65,536 each;
- a 64 KiB Core failure snapshot and disabled optimization remarks; and
- target-analysis arithmetic caps at the minimum meaningful values for those
  assumptions, with 100,000 graph entities and 100,000 solver updates.

Target-cost analysis remains nonfatal, as specified by the compilation facade.

The compiler library receives owned source text and returns an in-memory artifact;
it never searches the filesystem. The CLI alone reads the requested source and
writes files. The output root must either not exist or be a real empty directory.
Existing files, nonempty directories, and symlink roots are refused. The CLI
preflights every logical artifact path, writes to a fresh same-parent staging
directory in deterministic path order, and installs the complete staged directory.

The emitted ABI listing uses source function spellings, deterministic
declaration-order `function[index]` identities local to that compilation, entry
resources, and positional typed scoreboard homes. These dense indices are not a
stable package ABI.
