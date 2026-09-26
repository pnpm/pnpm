---
"pacquet": patch
---

`pnpm install` writes a workspace package that declares no dependencies into `pnpm-lock.yaml`, so `pnpm install --frozen-lockfile` can install it [#15875](https://github.com/pnpm/pnpm/issues/15875).
