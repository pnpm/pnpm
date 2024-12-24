---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

`pnpm update --filter <pattern> --latest <pkg>` should only change the specified package for the specified workspace, when `dedupe-peer-dependents` is set to `true` [#8877](https://github.com/pnpm/pnpm/issues/8877).
