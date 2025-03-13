---
"@pnpm/resolve-dependencies": major
"@pnpm/config": minor
"@pnpm/core": major
"@pnpm/types": minor
"@pnpm/headless": minor
"@pnpm/patching.config": minor
"@pnpm/patching.types": patch
"@pnpm/plugin-commands-script-runners": patch
"@pnpm/build-modules": minor
"pnpm": minor
---

Rename `pnpm.allowNonAppliedPatches` to `pnpm.allowUnusedPatches`. The old name is still supported but it would print a deprecation warning message.
