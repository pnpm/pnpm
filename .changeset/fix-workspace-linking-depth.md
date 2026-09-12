---
"pacquet": patch
---

`pnpm install` no longer links transitive dependencies with plain version ranges to workspace packages when `linkWorkspacePackages` is `true`, even with `preferWorkspacePackages` enabled. Use `linkWorkspacePackages: deep` to enable these links. Fixes [pnpm/pnpm#14781](https://github.com/pnpm/pnpm/issues/14781).
