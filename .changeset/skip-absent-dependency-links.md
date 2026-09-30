---
"@pnpm/deps.graph-builder": patch
"pnpm": patch
---

Fixed frozen installs creating symlinks to the working directory for skipped optional dependencies and unresolved peer dependencies [#16454](https://github.com/pnpm/pnpm/issues/16454).
