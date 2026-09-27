---
"pacquet": patch
---

A nested `packages` pattern in `pnpm-workspace.yaml`, such as `plugins/*/*`, no longer fails with `ERR_PNPM_WORKSPACE_INVALID_GLOB` on Windows when a managed directory — the store, cache, or state directory — is on a different volume than the workspace. Before, the cross-volume directory was turned into an unparseable glob, and the error named the user's own pattern as invalid [#16239](https://github.com/pnpm/pnpm/issues/16239).
