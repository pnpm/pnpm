---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` with `symlink` set to `false` and the isolated linker now fails with `ERR_PNPM_CONFIG_CONFLICT_SYMLINK_WITH_ISOLATED_LINKER` unless PnP is enabled. The combination previously left `node_modules` without direct dependencies and broke dependency builds.
