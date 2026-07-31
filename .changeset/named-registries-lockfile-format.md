---
"@pnpm/constants": minor
"@pnpm/deps.path": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/resolving.resolver-base": minor
"@pnpm/store.controller-types": minor
"@pnpm/lockfile.utils": major
"@pnpm/lockfile.fs": minor
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.deps-restorer": minor
"@pnpm/installing.context": minor
"@pnpm/deps.graph-builder": minor
"@pnpm/deps.compliance.license-scanner": minor
"@pnpm/deps.compliance.sbom": minor
"@pnpm/deps.compliance.commands": minor
"@pnpm/deps.inspection.tree-builder": minor
"@pnpm/deps.inspection.list": minor
"@pnpm/config.reader": minor
"@pnpm/installing.commands": minor
"pnpm": minor
"pacquet": minor
---

Added lockfile format 9.1, which records packages resolved from a named registry under registry-qualified keys (`<name>@<registryName>:<version>`, e.g. `foo@work:1.0.0`). This makes it possible to install the same package name — even the same version — from different registries in one project, which previously collided on a single `name@version` lockfile entry. Tarball URLs that follow the standard registry layout are no longer written to the lockfile for named-registry packages; they are recomputed from the `namedRegistries` setting on demand.

The 9.1 version is stamped only when the lockfile actually contains a named-registry package; other lockfiles stay on 9.0 byte for byte. pnpm 11 reads the 9.1 format unconditionally and writes it when the new `namedRegistriesLockfileFormat` setting is enabled — or when the existing lockfile is already on 9.1, so mixed-version teams don't flip formats back and forth. pnpm 12 writes the format by default.

Named registry aliases that shadow a reserved dependency specifier prefix (`file`, `link`, `workspace`, `runtime`, `npm`, `jsr`, ...) are now rejected with `ERR_PNPM_RESERVED_NAMED_REGISTRY_NAME` instead of being silently shadowed by the corresponding resolver.
