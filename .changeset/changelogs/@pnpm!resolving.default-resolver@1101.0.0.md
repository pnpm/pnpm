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
  - @pnpm/engine.runtime.bun-resolver@1102.0.17
  - @pnpm/engine.runtime.deno-resolver@1102.0.17
  - @pnpm/engine.runtime.node-resolver@1101.2.1
  - @pnpm/error@1100.1.3
  - @pnpm/hooks.types@1101.0.0
  - @pnpm/network.auth-header@1101.1.11
  - @pnpm/resolving.git-resolver@1100.1.18
  - @pnpm/resolving.local-resolver@1101.1.19
  - @pnpm/resolving.npm-resolver@1104.0.0
  - @pnpm/resolving.resolver-base@1101.1.1
  - @pnpm/resolving.tarball-resolver@1100.1.13
  - @pnpm/types@1102.0.0
