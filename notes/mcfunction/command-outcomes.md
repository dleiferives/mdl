# How Do Command Success, Result, and Missing Contexts Compose?

Status: **Measured on the official Java 26.2 dedicated server on 2026-07-18.**

Minecraft exposes command `success` and `result` as distinct channels. Neither is
equivalent to function continuation, and a store destination is not guaranteed to
be written merely because an `execute store` modifier was parsed.

## Measured matrix

The PS-1 fixture initialized selected destinations to `99`, ran this shape, and
queried the scores from later console roots:

```mcfunction
execute store success score #success mdl.ps1.out \
        store result score #result mdl.ps1.out \
        run <subject>
scoreboard players set #continued mdl.ps1.out 1
```

On Java 26.2:

| Subject | Success destination | Result destination | Outer function continues |
| --- | ---: | ---: | --- |
| Function completes without producing a command result | unchanged (`99`) | unchanged (`99`) | yes |
| Failing `data get` for a missing path | `0` | `0` | yes |
| Successful `data get` whose numeric value is zero | `1` | `0` | yes |
| Function executes `return 0` | `1` | `0` | yes |
| Function executes `return 7` | `1` | `7` | yes |
| `execute as` selects zero child contexts | unchanged (`99`) | unchanged (`99`) | yes |

The unchanged sentinels matter. A zero-context redirect does not synthesize a
failed result and does not invoke its attached store callbacks. Likewise, an
ordinary function that produces no command result leaves both destinations alone.
Both cases differ observably from a command that runs and fails, which writes zero
to both destinations.

`return 0` is successful command completion with numeric result zero. It must not be
modeled as failure merely because its result is false-like.

## Compiler consequences

- Model success availability, success value, result availability, result value,
  and outer continuation independently.
- Do not use an integer sentinel as the semantic representation of an unavailable
  channel; every scoreboard integer is a valid value. The sentinel above is only a
  black-box experiment proving whether vanilla performed a write.
- Do not collapse no child contexts, ordinary command failure, successful zero,
  explicit `return 0`, and abnormal sequence interruption.
- A language block does not acquire a native result merely because lowering outlines
  it into a function.

## Evidence

- Executable calibration:
  [`../../crates/mdl-test/tests/outcome_channels.rs`](../../crates/mdl-test/tests/outcome_channels.rs)
- Typed observation protocol:
  [`../compiler/pre-scheduler/ps-1-testing-plan.md`](../compiler/pre-scheduler/ps-1-testing-plan.md)
- Mojang's function-return description:
  <https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-3>
- Target server and pack format:
  <https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2>

The run used the official 26.2 server bundle SHA-256
`cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5`
and OpenJDK 25.0.3 without a connected client.
