---
"@pnpm/lockfile.utils": patch
"@pnpm/deps.compliance.sbom": patch
"pnpm": patch
"pacquet": patch
---

An `integrity` recorded on a git dependency's resolution (`resolution: {type: git, repo, commit, integrity: sha512-…}`) is no longer treated as a checksum. pnpm never verifies a git checkout against such a hash — the commit pins the content — so it is now dropped when the lockfile is rewritten, and `pnpm sbom` no longer republishes it as a CycloneDX/SPDX checksum. Lockfiles carrying one also load again instead of failing with `ERR_PNPM_BROKEN_LOCKFILE` [#13042](https://github.com/pnpm/pnpm/issues/13042).

`pnpm sbom` now also publishes the checksum of a `type: binary` runtime archive, which pnpm does verify.
