---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

When automatically installing missing peer dependencies, prefer versions that are already present in the direct dependencies of the root workspace package [#9835](https://github.com/pnpm/pnpm/pull/9835).
