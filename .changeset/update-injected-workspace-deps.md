---
"@pnpm/lockfile.utils": patch
"@pnpm/lockfile.verification": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now updates an injected workspace dependency after that package's own dependencies change, when `shared-workspace-lockfile` is `false` [#7209](https://github.com/pnpm/pnpm/issues/7209).
