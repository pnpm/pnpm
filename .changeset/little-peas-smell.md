---
"@pnpm/plugin-commands-installation": patch
"@pnpm/exec.build-commands": patch
---

Throws an error when the value provided by the `allow build` option overlaps with `ignoredBuildDependencies`.
