---
"pacquet": patch
---

The lockfile now always records an integrity for a registry dependency. A registry that publishes only the legacy `dist.shasum` — as `https://node-registry.bit.cloud/` does — had its packages recorded with a bare tarball URL and no hash, and `--lockfile-only` never computed one, so the very first `pnpm install --frozen-lockfile` failed with `ERR_PNPM_MISSING_TARBALL_INTEGRITY` [#13547](https://github.com/pnpm/pnpm/issues/13547). A `dist.shasum` is now promoted to its `sha1-` integrity as pnpm does, and a version that pins nothing at all has its tarball hashed even under `--lockfile-only`.
