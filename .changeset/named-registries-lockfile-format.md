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

Packages resolved from a named registry are now recorded in the lockfile under registry-qualified keys (`<name>@<registryName>:<version>`, e.g. `foo@work:1.0.0`). This makes it possible to install the same package name — even the same version — from different registries in one project. They previously collided on a single `name@version` entry, so whichever resolved first decided the tarball both consumers got.

The lockfile format version is unchanged. Registry-qualified keys appear only for packages resolved from a named registry, so a project that does not use `namedRegistries` sees no difference, and older pnpm versions keep reading the file.

Tarball URLs that follow the standard registry layout are no longer written to the lockfile for named-registry packages; they are recomputed from the `namedRegistries` setting on demand.

To use named registries, map your aliases in `pnpm-workspace.yaml`:

```yaml
namedRegistries:
  work: https://npm.enterprise.example.com/
```

Every alias the lockfile references must stay in `namedRegistries`: reading an entry whose alias is gone fails with `ERR_PNPM_MISSING_NAMED_REGISTRY` rather than silently falling back to the default registry, since that would fetch a different package. Renaming an alias re-resolves the packages that used it.

Named registry aliases that shadow a reserved dependency specifier prefix (`file`, `link`, `workspace`, `runtime`, `npm`, `jsr`, ...) are now rejected with `ERR_PNPM_RESERVED_NAMED_REGISTRY_NAME` instead of being silently shadowed by the corresponding resolver.

`pnpm licenses` and `pnpm sbom` now keep the two artifacts apart as well: license records carry the registry alias, and SBOM components carry the purl `repository_url` qualifier.
