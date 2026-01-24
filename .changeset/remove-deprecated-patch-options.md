---
"@pnpm/plugin-commands-installation": minor
"@pnpm/plugin-commands-deploy": minor
"@pnpm/plugin-commands-patching": major
"@pnpm/package-bins": minor
"@pnpm/patching.apply-patch": major
"@pnpm/patching.config": major
"@pnpm/patching.types": minor
"@pnpm/headless": minor
"@pnpm/build-modules": minor
"@pnpm/core": minor
"@pnpm/types": minor
"@pnpm/config": minor
"pnpm": minor
---

Removed the deprecated `allowNonAppliedPatches` completely in favor of `allowUnusedPatches`.
Remove `ignorePatchFailures` so all patch application failures should throw an error.
