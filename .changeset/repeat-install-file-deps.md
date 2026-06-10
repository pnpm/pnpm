---
"@pnpm/deps.status": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install` detects changes inside local file dependencies again. The optimistic repeat-install fast path only tracks manifest and lockfile modification times, so edits inside a local dependency's directory (or a repacked local tarball) were reported as "Already up to date". Projects with local file dependencies (`file:` and bare local path or tarball specifiers) now always run a full install, which refetches those dependencies, matching pnpm v10 behavior [#11795](https://github.com/pnpm/pnpm/issues/11795).
