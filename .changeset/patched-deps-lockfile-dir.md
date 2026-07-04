---
"@pnpm/lockfile.settings-checker": minor
"@pnpm/deps.status": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Relative paths in `patchedDependencies` are now resolved against the lockfile directory when computing patch file hashes, so running `pnpm install` from a subdirectory no longer fails with `ENOENT` looking for the patch file in the wrong location [#12762](https://github.com/pnpm/pnpm/pull/12762).
