---
"pacquet": patch
---

`pnpm sbom` and `pnpm licenses list` now resolve `namedRegistries` version prefixes in lockfile entries. `pnpm sbom` emits bare component versions with a sanitized `repository_url` PURL qualifier. `pnpm sbom` derives tarball download URLs from the named registry. `pnpm licenses list` reports packages from named registries as separate entries with bare versions and `registryName`.
