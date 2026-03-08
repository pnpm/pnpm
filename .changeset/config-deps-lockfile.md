---
"@pnpm/config.deps-installer": minor
"@pnpm/constants": patch
"@pnpm/types": patch
"pnpm": minor
---

Store config dependency integrity info in a separate `pnpm-config-lock.yaml` lockfile instead of inlining it in `pnpm-workspace.yaml`. The workspace manifest now contains only clean version specifiers for `configDependencies`, while the resolved versions, integrity hashes, and tarball URLs are recorded in the new lockfile. Projects using the old inline-hash format are automatically migrated on install.
