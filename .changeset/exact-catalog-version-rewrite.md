---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

Changing a catalog entry to a different exact version no longer re-resolves the dependency graph. The package is replaced in `pnpm-lock.yaml` directly, reusing the same check the `pnpm.overrides` fast path applies: every locked dependency of the package must still satisfy the new version's manifest. Installs fall back to a full resolution when anything other than the catalog reaches the package — an importer that depends on it directly, or another package that depends on it — since the graph would then need both versions.
