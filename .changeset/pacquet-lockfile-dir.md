---
"pacquet": minor
---

Added support for the `lockfileDir` setting and its `--lockfile-dir <dir>` flag on `pnpm install`, `add`, `update`, and `remove`. `pnpm-lock.yaml`, the root `node_modules` holding the virtual store, and the config dependencies now live in the given directory, each project is recorded under its path relative to it, and every project keeps its own `node_modules` of symlinks — so several projects can share one lockfile [#12042](https://github.com/pnpm/pnpm/issues/12042).
