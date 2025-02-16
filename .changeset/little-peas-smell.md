---
"@pnpm/plugin-commands-installation": patch
"@pnpm/exec.build-commands": patch
"pnpm": patch
---

Throws an error when the value provided by the `--allow-build` option overlaps with the `pnpm.ignoredBuildDependencies` list [#9105](https://github.com/pnpm/pnpm/pull/9105).
