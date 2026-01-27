---
"@pnpm/plugin-commands-installation": major
"@pnpm/plugin-commands-deploy": major
"@pnpm/plugin-commands-patching": major
"@pnpm/package-bins": major
"@pnpm/patching.apply-patch": major
"@pnpm/patching.config": major
"@pnpm/patching.types": major
"@pnpm/headless": major
"@pnpm/build-modules": major
"@pnpm/core": major
"@pnpm/types": major
"@pnpm/config": major
"pnpm": major
---

Removed the deprecated `allowNonAppliedPatches` completely in favor of `allowUnusedPatches`.
Remove `ignorePatchFailures` so all patch application failures should throw an error.
