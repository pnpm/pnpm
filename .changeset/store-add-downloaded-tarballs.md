---
"pacquet": minor
---

`pnpm store add` now prepares the store for offline installs from package tarballs downloaded some other way. After `pnpm store add ./tarballs/*.tgz`, `pnpm install --offline --frozen-lockfile` installs each registry package whose lockfile entry pins one of those tarballs [#2978](https://github.com/pnpm/pnpm/issues/2978).
