---
"@pnpm/plugin-commands-installation": patch
"@pnpm/exec.build-commands": patch
---

When the value provided by the `allow-build` option overlaps with `ignoredBuiltDependencies`, remove if from `ignoredBuiltDependencies` list.
