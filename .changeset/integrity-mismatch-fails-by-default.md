---
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"@pnpm/worker": patch
"pnpm": minor
---

Treat tarball-integrity mismatches against the lockfile as a hard failure by default. Previously, `pnpm install` (non-frozen) would log `ERR_PNPM_TARBALL_INTEGRITY`, silently re-resolve from the registry, and overwrite the locked integrity — which meant a compromised registry, proxy, or republished version could substitute attacker-controlled content on a clean machine even though the project shipped a committed lockfile.

`pnpm install` now exits with `ERR_PNPM_TARBALL_INTEGRITY` and a hint pointing at the new opt-in flag.

The only opt-in is **`pnpm install --update-checksums`** — narrowly scoped to refreshing the locked integrity values from what the registry currently serves. Mirrors yarn's flag of the same name. A warning still prints when the bypass takes effect so the operation is auditable.

`--force` and `pnpm update` deliberately do **not** bypass the integrity check. They are routine refresh operations; silently overwriting a locked integrity in those flows would erase the protection a committed lockfile is supposed to provide. `--frozen-lockfile` behavior is unchanged. `--fix-lockfile` keeps its documented purpose (filling in missing lockfile entries) and is also not a bypass.
