---
"@pnpm/lockfile.utils": patch
"pnpm": patch
---

Restored the heuristic that preserves tarball URLs in `pnpm-lock.yaml` when they cannot be derived from name+version+registry, even with the default `lockfileIncludeTarballUrl: false`. Without this, `pnpm install --frozen-lockfile` from an empty store fails with `ERR_PNPM_FETCH_404` for packages on registries that serve tarballs from a non-standard path — most notably GitHub Packages (`https://npm.pkg.github.com/download/<scope>/<name>/<version>/<hash>`) and JSR. `lockfileIncludeTarballUrl: true` continues to force the URL into the lockfile for every package [#11276](https://github.com/pnpm/pnpm/issues/11276).
