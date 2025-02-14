---
"@pnpm/plugin-commands-installation": patch
"@pnpm/exec.build-commands": patch
---

When the value provided by the `allow-build` option overlaps with `ignoredBuiltDependencies`, a prompt is provided to let the user decide whether to overwrite it.
