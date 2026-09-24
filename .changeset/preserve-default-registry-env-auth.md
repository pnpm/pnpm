---
"@pnpm/config.reader": patch
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Preserve the default registry when environment authentication configures tokens for multiple registries, including scoped registries. The lockfile resolution verifier also routes metadata queries to matching configured scoped registries for tarball URLs [pnpm/pnpm#15530](https://github.com/pnpm/pnpm/issues/15530).
