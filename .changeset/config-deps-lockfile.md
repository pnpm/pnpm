---
"@pnpm/config.deps-installer": minor
"@pnpm/constants": patch
"@pnpm/lockfile.fs": minor
"@pnpm/lockfile.types": minor
"@pnpm/lockfile.utils": minor
"@pnpm/types": patch
"@pnpm/tools.plugin-commands-self-updater": minor
"@pnpm/calc-dep-state": minor
"@pnpm/plugin-commands-setup": patch
"@pnpm/resolve-dependencies": patch
"pnpm": minor
---

Store config dependency and package manager integrity info in `pnpm-lock.yaml` instead of inlining it in `pnpm-workspace.yaml`. The workspace manifest now contains only clean version specifiers for `configDependencies`, while the resolved versions, integrity hashes, and tarball URLs are recorded in the lockfile as a separate YAML document. The env lockfile section also stores `packageManagerDependencies` resolved during version switching and self-update. Projects using the old inline-hash format are automatically migrated on install.
