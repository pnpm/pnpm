---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Don't crash when `auto-install-peers` is set to `true` and installation is done on a workspace with that has the same dependencies in multiple projects [#5454](https://github.com/pnpm/pnpm/issues/5454).
