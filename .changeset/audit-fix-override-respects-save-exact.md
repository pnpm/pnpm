---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

The interactive `pnpm audit --fix` picker now shows each patched version with the `saveExact` and `savePrefix` style that the override is written with [pnpm/pnpm#13209](https://github.com/pnpm/pnpm/issues/13209).
