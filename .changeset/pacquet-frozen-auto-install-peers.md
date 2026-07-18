---
"pacquet": patch
---

Fixed `pnpm install --frozen-lockfile` incorrectly failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a workspace project declares `peerDependencies` that `auto-install-peers` resolves. With `auto-install-peers` enabled (the default), pnpm records those missing peers in the lockfile importer's `dependencies`; the frozen-lockfile freshness check now folds `peerDependencies` into the comparison instead of reporting the materialized peers as removed.
