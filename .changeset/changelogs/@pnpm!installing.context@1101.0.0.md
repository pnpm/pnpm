## 1101.0.0

### Major Changes

- `node_modules/.modules.yaml` no longer records the registries an install resolved from, and the recorded copy is dropped from the file on the first install that rewrites it.

  It dated from the lockfile format that spelled a dependency's path relative to its registry, where reading an installed tree meant knowing the registries it was installed with. Dependency paths have not carried a registry for several major versions, and the recorded copy outlived its use: `pnpm list`, `pnpm why`, and single-project installs preferred it over the project's own configuration, so a project whose registry had changed since its last install was still read through the old one.

  They now use the configured registries, like every other command already did.

- The three registry lookups are now named for what they are keyed by, so that none of them is called `registries` — a name the `registries` setting itself has taken:

  | before | after |
  |---|---|
  | `Config.registries` | `Config.registriesByScope` |
  | `Config.namedRegistries` | `Config.registriesByPrefix` |
  | `Config.registryOptions` | `Config.registryOptionsByUrl` |

  The same rename applies to the `RegistryContext` fields, the `Registries` and `NamedRegistries` types (now `RegistriesByScope` and `RegistriesByPrefix`), `normalizeRegistries` / `normalizeNamedRegistries` (now `normalizeRegistriesByScope` / `normalizeRegistriesByPrefix`), and the `BUILTIN_NAMED_REGISTRIES` constant (now `BUILTIN_REGISTRIES_BY_PREFIX`).

  This is an internal rename: no setting, error code, lockfile field, or `.pnpmfile.cjs` hook field changes. A `preResolution` hook still reads `ctx.registries`, which is the name pacquet passes as well. The `registries` and `namedRegistries` settings are read under the names users write them.

  The pnpr resolve request sends `registriesByPrefix` where it sent `namedRegistries`. A pnpr server and its clients must be on matching versions, which is already the case for an experimental server.

### Patch Changes

- Updated dependencies:
  - @pnpm/constants@1102.0.0
  - @pnpm/core-loggers@1100.3.3
  - @pnpm/error@1100.1.3
  - @pnpm/installing.modules-yaml@1101.0.0
  - @pnpm/installing.read-projects-context@1101.0.0
  - @pnpm/lockfile.fs@1100.2.3
  - @pnpm/resolving.resolver-base@1101.1.1
  - @pnpm/store.controller@1102.0.13
  - @pnpm/types@1102.0.0
