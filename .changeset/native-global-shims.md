---
"pacquet": minor
---

Context-aware global shims are now native executables on every platform: each bin that `globalShims` enables (`node`, `deno`, `bun`, and packages added with `pnpm shim add`) is the pnpm executable published under the bin's name, with its global target recorded in a sidecar file beside it. No shell runs between the caller and the target, so environment variables whose names are not valid shell identifiers reach every such command, not only `node`. On Windows the `.cmd` and `.ps1` shims for these bins are replaced by `<name>.exe`. Shims written by earlier pnpm 12 releases are migrated on the next global install or self-update, and the `.pnpm-shim-v1` dispatcher executable is removed.
