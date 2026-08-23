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
  - @pnpm/deps.github-actions@1100.1.6
  - @pnpm/deps.inspection.list@1101.0.0
  - @pnpm/deps.inspection.outdated@1100.1.26
  - @pnpm/deps.inspection.peers-checker@1100.0.29
  - @pnpm/deps.inspection.peers-issues-renderer@1100.0.13
  - @pnpm/error@1100.1.3
  - @pnpm/global.commands@1101.0.0
  - @pnpm/global.packages@1101.0.0
  - @pnpm/installing.modules-yaml@1101.0.0
  - @pnpm/lockfile.fs@1100.2.3
  - @pnpm/network.auth-header@1101.1.11
  - @pnpm/network.fetch@1100.1.13
  - @pnpm/resolving.default-resolver@1101.0.0
  - @pnpm/resolving.npm-resolver@1104.0.0
  - @pnpm/resolving.registry.types@1100.1.10
  - @pnpm/store.path@1100.0.6
  - @pnpm/types@1102.0.0
  - @pnpm/workspace.project-manifest-reader@1100.0.25
