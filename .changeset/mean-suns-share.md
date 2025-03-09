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

Deprecate `pnpm.allowNonAppliedPatches` and introduce `pnpm.strictPatches`.

Unlike `allowNonAppliedPatches` which only determines whether pnpm should throw an error when a patch is found unused and doesn't concern itself with whether patch fails to apply, `strictPatches` manages both unused patches and patch application failures.

If `strictPatches` is set to `true`, unused patches will result in error and patch application failures will not be ignored. If `strictPatches` is set to `false`, unused patches are allowed and patch application failures will be ignored.
