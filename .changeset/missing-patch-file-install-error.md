---
"@pnpm/lockfile.settings-checker": patch
"@pnpm/deps.status": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm install` now fails with `ERR_PNPM_PATCH_NOT_FOUND` when a patch file listed in `patchedDependencies` does not exist. It used to fail with a raw `ENOENT` error and a stack trace [#5268](https://github.com/pnpm/pnpm/issues/5268).
