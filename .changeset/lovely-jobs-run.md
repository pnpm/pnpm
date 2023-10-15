---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

When `shared-workspace-lockfile` is set to `false`, read the pnpm settings from `package.json` files that are nested. This was broken in pnpm v8.9.0 [#7184](https://github.com/pnpm/pnpm/issues/7184).
