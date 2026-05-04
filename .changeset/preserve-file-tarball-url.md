---
"@pnpm/lockfile.utils": patch
"pnpm": patch
---

Fix `ERR_PNPM_FETCH_404` when installing a project whose lockfile depends on a `file:` tarball. The previous behavior dropped the `tarball` field from `file:` and git-hosted resolutions when `lockfile-include-tarball-url=false` (the default), even though those URLs cannot be reconstructed from the package name, version, and registry [#11407](https://github.com/pnpm/pnpm/issues/11407).
