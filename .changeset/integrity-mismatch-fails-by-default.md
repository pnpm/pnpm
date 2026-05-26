---
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"@pnpm/worker": patch
"pnpm": minor
---

Treat tarball-integrity mismatches against the lockfile as a hard failure by default. Previously, `pnpm install` (non-frozen) would log `ERR_PNPM_TARBALL_INTEGRITY`, silently re-resolve from the registry, and overwrite the locked integrity — which meant a compromised registry, proxy, or republished version could substitute attacker-controlled content on a clean machine even though the project shipped a committed lockfile.

`pnpm install` now exits with the original `ERR_PNPM_TARBALL_INTEGRITY` and a hint pointing at the recovery flags. To refresh the locked integrity from the registry, run:

- `pnpm install --update-checksums` — new flag, narrowly scoped to refreshing checksums. Mirrors yarn's `--update-checksums`.
- `pnpm install --force` — broader refresh.
- `pnpm update` — re-resolves and refreshes everything (or a targeted subset).

`--frozen-lockfile` behavior is unchanged. `--fix-lockfile` keeps its documented purpose (filling in missing lockfile entries) and deliberately does not bypass the integrity check.
