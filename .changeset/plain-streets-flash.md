---
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

Fixed the `time` field in `pnpm-lock.yaml` being entirely wiped on every install. An incorrect dependency path format was used when matching entries to prune, causing all entries to always be removed.
