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
  - @pnpm/config.pick-registry-for-package@1101.0.0
  - @pnpm/config.reader@1102.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/network.auth-header@1101.1.11
  - @pnpm/network.fetch@1100.1.13
  - @pnpm/network.web-auth@1101.4.3
  - @pnpm/registry-access.client@1100.1.15
  - @pnpm/resolving.registry.types@1100.1.10
  - @pnpm/types@1102.0.0
