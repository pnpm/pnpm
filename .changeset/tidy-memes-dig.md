---
"@pnpm/plugin-commands-installation": patch
pnpm: patch
---

If there is no pnpm related configuration in `package.json`, `onlyBuiltDependencies` will be written to `pnpm-workspace.yaml` file [#9404](https://github.com/pnpm/pnpm/pull/9404).
