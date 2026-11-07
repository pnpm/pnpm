---
"pacquet": patch
---

The sync of an injected workspace dependency now copies a file whose source has reached the filesystem's limit on hard links, as `pnpm install` does for a store file. `pnpm patch-commit` now copies a file into its scratch directory whenever the filesystem refuses to hard link it.
