---
"@pnpm/resolving.local-resolver": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

`catalogMode` and `--save-catalog` no longer move a local path, tarball, or `workspace:<path>` specifier into a catalog. Such a specifier is resolved against the project that declares it, so one catalog entry cannot mean the same directory for every project that references it [#14437](https://github.com/pnpm/pnpm/issues/14437).
