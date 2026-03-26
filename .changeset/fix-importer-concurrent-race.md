---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Fixed a race condition where concurrent package imports could destroy each other's files. The fast path now uses atomic `mkdirSync` to claim the target directory instead of `makeEmptyDirSync`, which would empty a directory being written by another process. When the directory already exists (concurrent import or re-import), the staging path is used for a full atomic directory replacement.
