---
"@pnpm/installing.deps-installer": patch
"@pnpm/worker": patch
"pnpm": patch
---

Treat tarball-integrity mismatches against the lockfile as a hard failure by default. Previously, `pnpm install` (non-frozen) would log `ERR_PNPM_TARBALL_INTEGRITY`, silently re-resolve from the registry, and overwrite the locked integrity — which meant a compromised registry, proxy, or republished version could substitute attacker-controlled content on a clean machine even though the project shipped a committed lockfile.

`pnpm install` now exits with the original `ERR_PNPM_TARBALL_INTEGRITY` and a hint pointing at the recovery flags. Callers who do want pnpm to refresh the locked integrity from the registry must opt in explicitly with `--fix-lockfile`, `--force`, or `pnpm update`. `--frozen-lockfile` behavior is unchanged.
