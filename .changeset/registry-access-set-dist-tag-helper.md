---
"@pnpm/registry-access.commands": patch
"@pnpm/auth.commands": patch
---

Refactor the dist-tag-add and login (classic adduser) handlers to delegate their PUTs to two new small shared packages — `@pnpm/registry-access.set-dist-tag` and `@pnpm/registry-access.add-user`. Downstream tests in this monorepo now use these helpers (via `@pnpm/testing.registry-mock`) instead of `addDistTag` / `addUser` from `@pnpm/registry-mock`, which relied on the unmaintained `anonymous-npm-registry-client`.
