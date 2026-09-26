---
"pacquet": minor
---

`pnpm store add` now makes a local package tarball available to offline installs of the registry package it holds. Running `pnpm store add ./tarballs/*.tgz` warms the store for `pnpm install --offline` when the lockfile records the same integrity as those tarballs [#2978](https://github.com/pnpm/pnpm/issues/2978).
