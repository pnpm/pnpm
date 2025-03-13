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

Add `pnpm.ignorePatchFailures` to manage whether pnpm would ignore patch application failures.

If `ignorePatchFailures` is not set, pnpm would throw an error when patches with exact versions or version ranges fail to apply, and it would ignore failures from name-only patches.

If `ignorePatchFailures` is explicitly set to `false`, pnpm would throw an error when any type of patch fails to apply.

If `ignorePatchFailures` is explicitly set to `true`, pnpm would print a warning when any type of patch fails to apply.
