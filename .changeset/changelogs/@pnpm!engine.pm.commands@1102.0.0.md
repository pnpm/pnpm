## 1102.0.0

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

- `pnpm self-update` now rewrites a simple `devEngines.packageManager.version` range (`^`/`~`) to the newly installed version, keeping the operator — matching how `pnpm update` and `pnpm runtime set` rewrite ranges. Complex ranges such as `>=8.0.0` that the new version satisfies are still left unchanged [#13935](https://github.com/pnpm/pnpm/issues/13935).

- `pnpm self-update <tag>` no longer downgrades when the dist-tag points at the pnpm version already running and that version is younger than `minimumReleaseAge`. The maturity cutoff moved the tag back to the previous mature release, so `pnpm self-update next-12` on v12.0.0-rc.4 switched to v12.0.0-rc.3.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.28
  - @pnpm/building.policy@1100.0.20
  - @pnpm/cli.meta@1100.0.15
  - @pnpm/cli.utils@1101.0.24
  - @pnpm/config.pick-registry-for-package@1101.0.0
  - @pnpm/config.reader@1102.0.0
  - @pnpm/config.version-policy@1100.2.1
  - @pnpm/deps.graph-hasher@1100.2.18
  - @pnpm/deps.security.signatures@1102.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/global.commands@1101.0.0
  - @pnpm/global.packages@1101.0.0
  - @pnpm/installing.client@1100.3.5
  - @pnpm/installing.deps-restorer@1103.0.0
  - @pnpm/installing.env-installer@1103.0.0
  - @pnpm/lockfile.fs@1100.2.3
  - @pnpm/lockfile.types@1100.0.20
  - @pnpm/network.auth-header@1101.1.11
  - @pnpm/pkg-manifest.utils@1100.4.1
  - @pnpm/resolving.npm-resolver@1104.0.0
  - @pnpm/store.connection-manager@1101.0.0
  - @pnpm/store.controller@1102.0.13
  - @pnpm/types@1102.0.0
  - @pnpm/workspace.project-manifest-reader@1100.0.25
