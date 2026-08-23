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

- Fixed `pnpm patch-commit` in project and edit paths containing non-ASCII characters.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.24
  - @pnpm/config.reader@1102.0.0
  - @pnpm/config.writer@1100.0.23
  - @pnpm/constants@1102.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/installing.commands@1101.0.0
  - @pnpm/installing.modules-yaml@1101.0.0
  - @pnpm/lockfile.fs@1100.2.3
  - @pnpm/lockfile.utils@1102.0.0
  - @pnpm/patching.apply-patch@1100.0.7
  - @pnpm/pkg-manifest.reader@1100.0.17
  - @pnpm/store.connection-manager@1101.0.0
  - @pnpm/store.path@1100.0.6
  - @pnpm/types@1102.0.0
  - @pnpm/workspace.project-manifest-reader@1100.0.25
  - @pnpm/workspace.workspace-manifest-reader@1100.1.7
