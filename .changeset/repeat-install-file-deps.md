---
"@pnpm/deps.status": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install` detects changes inside `file:` dependencies again. The optimistic repeat-install fast path only tracks manifest and lockfile modification times, so edits inside a `file:` dependency's directory (or a repacked `file:` tarball) were reported as "Already up to date". Projects with `file:` dependencies now always run a full install, which refetches those dependencies, matching pnpm v10 behavior [#11795](https://github.com/pnpm/pnpm/issues/11795).
