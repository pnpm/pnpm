---
"pacquet": patch
---

`pnpm sbom` and `pnpm licenses list` now resolve `namedRegistries` version prefixes in lockfile entries. `pnpm sbom` strips the registry alias from component versions, adds the sanitized `repository_url` PURL qualifier, and derives tarball download URLs from the named registry. `pnpm licenses list` reports packages from named registries separately with their bare versions and `registryName`.
