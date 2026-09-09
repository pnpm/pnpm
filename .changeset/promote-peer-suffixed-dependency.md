---
"pacquet": patch
---

`pnpm install` and `pnpm add` no longer write a broken importer entry when a package that `pnpm-lock.yaml` already holds as a transitive dependency with resolved peer dependencies is added as a direct dependency. pnpm now resolves the new entry, so it records the peer-suffixed version that has a snapshot. It previously skipped resolution and recorded the bare version, which had no snapshot and left the package a dangling symlink in `node_modules`.
