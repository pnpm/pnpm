---
"@pnpm/building.after-install": major
"@pnpm/building.commands": major
"@pnpm/config.commands": major
"@pnpm/config.normalize-registries": major
"@pnpm/config.pick-registry-for-package": major
"@pnpm/config.reader": major
"@pnpm/constants": major
"@pnpm/deps.compliance.commands": major
"@pnpm/deps.compliance.license-scanner": major
"@pnpm/deps.compliance.sbom": major
"@pnpm/deps.graph-builder": major
"@pnpm/deps.inspection.commands": major
"@pnpm/deps.inspection.list": major
"@pnpm/deps.inspection.tree-builder": major
"@pnpm/deps.path": major
"@pnpm/deps.security.signatures": major
"@pnpm/engine.pm.commands": major
"@pnpm/engine.runtime.commands": major
"@pnpm/exec.commands": major
"@pnpm/global.commands": major
"@pnpm/global.packages": major
"@pnpm/hooks.types": major
"@pnpm/installing.commands": major
"@pnpm/installing.context": major
"@pnpm/installing.deps-installer": major
"@pnpm/installing.deps-resolver": major
"@pnpm/installing.deps-restorer": major
"@pnpm/installing.env-installer": major
"@pnpm/installing.modules-yaml": major
"@pnpm/installing.read-projects-context": major
"@pnpm/lockfile.to-pnp": major
"@pnpm/lockfile.utils": major
"@pnpm/patching.commands": major
"@pnpm/pnpr.client": major
"@pnpm/registry-access.commands": major
"@pnpm/releasing.commands": major
"@pnpm/resolving.default-resolver": major
"@pnpm/resolving.npm-resolver": major
"@pnpm/store.commands": major
"@pnpm/store.connection-manager": major
"@pnpm/testing.command-defaults": major
"@pnpm/testing.temp-store": major
"@pnpm/types": major
"pacquet": patch
---

The three registry lookups are now named for what they are keyed by, so that none of them is called `registries` — a name the `registries` setting itself has taken:

| before | after |
|---|---|
| `Config.registries` | `Config.registriesByScope` |
| `Config.namedRegistries` | `Config.registriesByPrefix` |
| `Config.registryOptions` | `Config.registryOptionsByUrl` |

The same rename applies to the `RegistryContext` fields, the `Registries` and `NamedRegistries` types (now `RegistriesByScope` and `RegistriesByPrefix`), `normalizeRegistries` / `normalizeNamedRegistries` (now `normalizeRegistriesByScope` / `normalizeRegistriesByPrefix`), and the `BUILTIN_NAMED_REGISTRIES` constant (now `BUILTIN_REGISTRIES_BY_PREFIX`).

This is an internal rename: no setting, error code, lockfile field, or `.pnpmfile.cjs` hook field changes. A `preResolution` hook still reads `ctx.registries`, which is the name pacquet passes as well. The `registries` and `namedRegistries` settings are read under the names users write them.

The pnpr resolve request sends `registriesByPrefix` where it sent `namedRegistries`. A pnpr server and its clients must be on matching versions, which is already the case for an experimental server.
