---
"pacquet": patch
---

`pnpm config get globalShims`, `pnpm shim list`, and global installs no longer read `globalShims` from a project's `pnpm-workspace.yaml`. Only the global config file, the pnpm home's own `pnpm-workspace.yaml`, and `PNPM_CONFIG_GLOBAL_SHIMS` set it, so a repository cannot choose which globally installed packages get project-aware shims.
