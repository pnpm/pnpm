## 1101.0.0

### Major Changes

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
  - @pnpm/cli.utils@1101.0.24
  - @pnpm/config.normalize-registries@1101.0.0
  - @pnpm/config.reader@1102.0.0
  - @pnpm/crypto.integrity@1100.0.5
  - @pnpm/deps.path@1101.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/global.packages@1101.0.0
  - @pnpm/installing.client@1100.3.5
  - @pnpm/installing.context@1101.0.0
  - @pnpm/lockfile.types@1100.0.20
  - @pnpm/lockfile.utils@1102.0.0
  - @pnpm/store.cafs@1100.2.0
  - @pnpm/store.connection-manager@1101.0.0
  - @pnpm/store.controller-types@1101.1.2
  - @pnpm/store.index@1100.2.5
  - @pnpm/store.path@1100.0.6
  - @pnpm/types@1102.0.0
