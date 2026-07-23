---
"pacquet": patch
---

`pnpm install`, `pnpm add`, `pnpm update`, and `pnpm remove` now support recursive (`-r`) and filtered (`--filter`) execution in workspaces configured with one lockfile per project (`sharedWorkspaceLockfile: false`), instead of failing with `ERR_PNPM_RECURSIVE_SHARED_LOCKFILE_UNSUPPORTED`. Each selected project is installed independently against its own `pnpm-lock.yaml`, `node_modules`, and virtual store, matching pnpm.
