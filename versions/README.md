# Minecraft conformance artifacts

Official Minecraft server JARs and the runtime `libraries/` they extract are local
test inputs, not repository source. They are intentionally ignored.

Stage 7 targets Java Edition 26.2 under Java 25. Set `MDL_SERVER_JAR` to Mojang's
official 26.2 server bundle and `MDL_JAVA` to a Java 25 executable before running the
ignored server suites. The harness verifies behavior on the real server; compiler
target facts remain in `TargetSpec` and do not depend on a machine-specific path.

The July 14, 2026 Stage 7 run used distribution-bundle SHA-256
`cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5`;
its extracted server payload had SHA-256
`183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8`.
Mojang's 26.2 version metadata identifies the distribution object by SHA-1
`823e2250d24b3ddac457a60c92a6a941943fcd6a` and size `60,894,273` bytes.

Reference: <https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2>

- Version metadata:
  <https://piston-meta.mojang.com/v1/packages/c8eb00be8a1f9fb9adf70ee415b7e1f746b636e8/26.2.json>
- Pinned official server object:
  <https://piston-data.mojang.com/v1/objects/823e2250d24b3ddac457a60c92a6a941943fcd6a/server.jar>
