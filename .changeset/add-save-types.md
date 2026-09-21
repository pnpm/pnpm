---
"pacquet": minor
---

Added `pnpm add --save-types` to save available `@types/*` packages in `devDependencies` alongside registry dependencies. Packages that declare bundled TypeScript types are skipped. Set `saveTypes: true` in `pnpm-workspace.yaml` to enable this by default [pnpm/pnpm#3868](https://github.com/pnpm/pnpm/issues/3868).
