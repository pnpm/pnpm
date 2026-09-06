---
"pacquet": patch
---

pnpm no longer rewrites the `packageManagerDependencies` block of `pnpm-lock.yaml` back and forth when package manager version switching is turned off. `pnpm install` recorded the pinned pnpm version there and commands such as `pnpm list` did not [#14575](https://github.com/pnpm/pnpm/issues/14575).
