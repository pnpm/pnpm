---
"@pnpm/audit": minor
"@pnpm/plugin-commands-audit": minor
"pnpm": minor
---

The `pnpm audit` command now also audits dependencies from `pnpm-lock.yaml`, including `configDependencies` and `packageManagerDependencies` along with their transitive dependencies.
