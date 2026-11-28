---
"pnpm": patch
---

`pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` when a project depends on a workspace project whose manifest is `package.yaml` or `package.json5`. It failed when the install was filtered with `--filter`, or when `sharedWorkspaceLockfile` was `false`.
