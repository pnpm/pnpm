---
"@pnpm/config.reader": patch
"pnpm": patch
---

No longer warn about using both `packageManager` and `devEngines.packageManager` when the two fields pin the same package manager at the same version with the same integrity hash (e.g. both `pnpm@11.5.1+sha512.…`). Previously the hash was stripped from the legacy `packageManager` field but not from `devEngines.packageManager`, so even identical specifications looked like a mismatch [#12028](https://github.com/pnpm/pnpm/issues/12028).

The warning still fires on any genuine divergence, and several cases now state the specific reason instead of a single generic message: a different package manager, a different version, or contradictory integrity hashes for the same version.
