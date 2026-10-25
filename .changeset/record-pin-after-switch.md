---
"pnpm": patch
---

The pnpm version that `packageManager` pins now records that pin in a fresh `pnpm-lock.yaml` when pnpm 10 switched to it. A later `pnpm install --frozen-lockfile` then succeeds [#17303](https://github.com/pnpm/pnpm/issues/17303).
