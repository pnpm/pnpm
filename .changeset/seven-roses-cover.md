---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Don't fail on projects with linked dependencies, when `auto-install-peers` is set to `true` [#4796](https://github.com/pnpm/pnpm/issues/4796).
