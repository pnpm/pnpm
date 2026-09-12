---
"pacquet": patch
---

Fixed shell emulator not expanding `${VAR}` parameter expansions, such as `${MY_VAR}` and `${MY_VAR:-default}`, when `shellEmulator` is enabled [#14814](https://github.com/pnpm/pnpm/issues/14814).
