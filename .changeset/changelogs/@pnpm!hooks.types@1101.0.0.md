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

- A custom fetcher can no longer replace the archive integrity that `pnpm-lock.yaml` pins: the locked value is restored after a `canFetch` or `fetch` hook rewrites the resolution, and delegating a locked archive to a directory or git source now fails instead of installing unverified content.

  The Rust CLI now also loads the pnpmfiles named by the `pnpmfile` setting (a single path or an ordered list), and hands custom fetchers native `localTarball` and `remoteTarball` callbacks — including on a fresh install that has to compute a missing tarball integrity, which is then reused by later offline installs. File maps a fetcher returns are accepted only when they match what those native callbacks extracted.

- Updated dependencies:
  - @pnpm/fetching.fetcher-base@1100.2.8
  - @pnpm/lockfile.types@1100.0.20
  - @pnpm/resolving.resolver-base@1101.1.1
  - @pnpm/types@1102.0.0
