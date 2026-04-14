---
"@pnpm/config.reader": minor
"@pnpm/installing.commands": minor
"pnpm": minor
---

Added a new `lockfileStorage: split` setting that stores per-package lockfiles while using the fast shared resolution path. This merges per-package lockfiles into a temporary unified lockfile for resolution via `mutateModules()`, then splits the result back into per-package files. This provides per-package lockfiles that reduce git merge conflicts and improve CI cache granularity in large monorepos, without the performance penalty of `sharedWorkspaceLockfile: false`.
