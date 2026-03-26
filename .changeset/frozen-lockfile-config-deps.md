---
"@pnpm/installing.env-installer": minor
"pnpm": minor
---

Throws `FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` when attempting to install configuration dependencies with `--frozen-lockfile` active and the env lockfile is missing or out-of-date. Previously, the operation would silently rewrite the workspace file or resolve in-memory.
