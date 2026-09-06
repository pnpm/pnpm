---
"pacquet": patch
---

Commands other than `pnpm install` now record the pinned pnpm version in `pnpm-lock.yaml` when package manager version switching is turned off. Alternating an install with any other command rewrote the same `packageManagerDependencies` lines back and forth [#14575](https://github.com/pnpm/pnpm/issues/14575).
