---
"@pnpm/lockfile.utils": patch
"pnpm": patch
---

Restored the heuristic that preserves non-reconstructable tarball URLs in `pnpm-lock.yaml` even when `lockfile-include-tarball-url` is `false` (the default). Without this, `pnpm install --frozen-lockfile` from an empty store fails with `ERR_PNPM_FETCH_404` for packages on registries that serve tarballs from a non-standard path — most notably GitHub Packages (`https://npm.pkg.github.com/download/<scope>/<name>/<version>/<hash>`) and JSR. `lockfile-include-tarball-url: true` continues to force the URL into the lockfile for every package [#11276](https://github.com/pnpm/pnpm/issues/11276).
