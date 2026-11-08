---
"pacquet": patch
---

The sync of an injected workspace dependency now copies a file whose source has reached the filesystem's limit on hard links. `pnpm patch-commit` now copies a file into its scratch directory when the filesystem refuses to hard link it, unless the path is missing or already occupied [pnpm/tasks#283](https://github.com/pnpm/tasks/issues/283).
