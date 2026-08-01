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

Added lockfile format 12.0, which records packages resolved from a named registry under registry-qualified keys (`<name>@<registryName>:<version>`, e.g. `foo@work:1.0.0`). This makes it possible to install the same package name — even the same version — from different registries in one project, which previously collided on a single `name@version` lockfile entry. Tarball URLs that follow the standard registry layout are no longer written to the lockfile for named-registry packages; they are recomputed from the `namedRegistries` setting on demand.

pnpm 11 reads the 12.0 format unconditionally and writes it when the new `namedRegistriesLockfileFormat` setting is enabled — or when the existing lockfile is already on 12.0, so mixed-version teams don't flip formats back and forth. Enabling the format on pnpm 11 migrates an existing lockfile with a full resolution so transitive named-registry dependencies are also qualified. pnpm 12 writes the format by default while continuing to accept 9.x lockfiles unchanged during frozen installs.

To use the format on pnpm 11, map your aliases in `pnpm-workspace.yaml` and turn the setting on:

```yaml
namedRegistries:
  work: https://npm.enterprise.example.com/
namedRegistriesLockfileFormat: true
```

Every alias a 12.0 lockfile references must stay in `namedRegistries`: reading an entry whose alias is gone fails with `ERR_PNPM_MISSING_NAMED_REGISTRY` rather than silently falling back to the default registry, since that would fetch a different package. Renaming an alias re-resolves the packages that used it. With the setting off, pnpm 11 keeps the previous behavior, where two registries serving the same name and version still share one lockfile entry.

Named registry aliases that shadow a reserved dependency specifier prefix (`file`, `link`, `workspace`, `runtime`, `npm`, `jsr`, ...) are now rejected with `ERR_PNPM_RESERVED_NAMED_REGISTRY_NAME` instead of being silently shadowed by the corresponding resolver.

`pnpm licenses` and `pnpm sbom` now keep the two artifacts apart as well: license records carry the registry alias, and SBOM components carry the purl `repository_url` qualifier.
