---
"@pnpm/core": patch
pnpm: patch
---

Fix a bug causing catalog snapshots to be removed from the `pnpm-lock.yaml` file when using `--fix-lockfile` and `--filter`. [#8639](https://github.com/pnpm/pnpm/issues/8639)
