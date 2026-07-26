---
"pacquet": patch
---

`$dep-name` self-references in `overrides` are now resolved against the root manifest's direct dependencies, so an override such as `rolldown: $rolldown` records the concrete specifier in `pnpm-lock.yaml` and no longer fails a frozen install with `ERR_PNPM_OUTDATED_LOCKFILE` [#13314](https://github.com/pnpm/pnpm/issues/13314). A reference to a package that is not a direct dependency fails with `ERR_PNPM_CANNOT_RESOLVE_OVERRIDE_VERSION`, and the deprecated syntax now warns, pointing at catalogs.
