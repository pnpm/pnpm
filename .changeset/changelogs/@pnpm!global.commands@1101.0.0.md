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

- Fixed `pnpm update --global --latest` failing with a 404 error when a globally installed package was not added from the registry by name. Packages installed from a local path (`link:`/`file:`), a git repository, a tarball URL, an `npm:` alias, or a named registry now keep their spec during a global update instead of being looked up by name in the default registry. See pnpm/pnpm#12854.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.28
  - @pnpm/bins.remover@1100.0.21
  - @pnpm/bins.resolver@1100.0.15
  - @pnpm/cli.utils@1101.0.24
  - @pnpm/config.reader@1102.0.0
  - @pnpm/core-loggers@1100.3.3
  - @pnpm/deps.inspection.list@1101.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/global.packages@1101.0.0
  - @pnpm/installing.deps-installer@1104.0.0
  - @pnpm/pkg-manifest.reader@1100.0.17
  - @pnpm/pkg-manifest.utils@1100.4.1
  - @pnpm/store.connection-manager@1101.0.0
  - @pnpm/types@1102.0.0
