---
"@pnpm/hooks.read-package-hook": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

A parent-scoped override (`parent>peer`) in `overrides` now overrides a `peerDependencies` entry of `parent` with the specified version [#5391](https://github.com/pnpm/pnpm/issues/5391).
