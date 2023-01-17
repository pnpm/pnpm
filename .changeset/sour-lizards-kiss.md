---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

`pnpm install --fix-lockfile` should not fail if the package has no dependencies [#5878](https://github.com/pnpm/pnpm/issues/5878).
