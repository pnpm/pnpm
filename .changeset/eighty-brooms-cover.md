---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

Don't remove automatically installed peer dependencies from the root workspace project, when `dedupe-peer-dependents` is `true` [#6154](https://github.com/pnpm/pnpm/issues/6154).
