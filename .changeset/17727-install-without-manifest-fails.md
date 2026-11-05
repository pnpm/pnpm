---
"pacquet": patch
---

`pnpm install`, `pnpm ci`, and `pnpm install-test` outside a workspace now fail with `ERR_PNPM_NO_PKG_MANIFEST` when the directory has no `package.json`. They no longer create a `package.json`, a lockfile, and `node_modules` there [#17727](https://github.com/pnpm/pnpm/issues/17727).
