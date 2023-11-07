---
"@pnpm/store-controller-types": patch
"@pnpm/plugin-commands-store": patch
"@pnpm/package-store": patch
"pnpm": patch
---

When using `pnpm store prune --force` alien directories are removed from the store [#7272](https://github.com/pnpm/pnpm/pull/7272).
