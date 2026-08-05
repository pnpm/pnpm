---
"pacquet": patch
---

A registry dependency is now always recorded in `pnpm-lock.yaml` with an integrity hash, including under `--lockfile-only`. Packages from a registry that publishes no subresource-integrity metadata — `https://node-registry.bit.cloud/`, for one — were recorded without one, so the next `pnpm install --frozen-lockfile` failed with `ERR_PNPM_MISSING_TARBALL_INTEGRITY` [#13547](https://github.com/pnpm/pnpm/issues/13547).
