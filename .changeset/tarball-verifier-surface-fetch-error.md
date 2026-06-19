---
"pnpm": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/installing.deps-installer": patch
---

The lockfile tarball URL verification no longer reports a registry metadata fetch failure (for example a `403`/`401` on a private registry, or a network error) as `ERR_PNPM_TARBALL_URL_MISMATCH`. The underlying error is now surfaced in the message, and a transport failure is reported under a distinct `ERR_PNPM_TARBALL_URL_FETCH_FAILED` code so that `ERR_PNPM_TARBALL_URL_MISMATCH` keeps its "this looks like tampering" meaning. The install error hint for these failures now points to registry credentials and connectivity instead of the lockfile.
