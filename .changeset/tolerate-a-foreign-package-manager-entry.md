---
"pacquet": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` no longer fails when `pnpm-lock.yaml` records the pinned pnpm version alongside an engine package the running pnpm does not install it from. An entry pinning another version is still refused, and a plain install rewrites the block [#14124](https://github.com/pnpm/pnpm/issues/14124).
