---
"@pnpm/resolving.local-resolver": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

`catalogMode` and `--save-catalog` no longer move a local path, tarball, or `workspace:<path>` specifier into a catalog — such a path is resolved against the project that declares it, so a shared catalog entry cannot mean the same directory for every project referencing it [#14437](https://github.com/pnpm/pnpm/issues/14437).
