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

- `pnpm add --allow-build` now adds to the `allowBuilds` entries already in `pnpm-workspace.yaml` instead of replacing them [#13872](https://github.com/pnpm/pnpm/issues/13872).

- Fix recursive `pnpm update <name>@<version>` so an exact pinned update stays scoped to the requested version line: copies of the same package on another major line — or, for a `0.x` request, another minor line — keep their locked resolution instead of being re-resolved along with the target.

- `pnpm remove` now prunes undecided entries (`"set this to true or false"`) from `allowBuilds` in `pnpm-workspace.yaml` when `sharedWorkspaceLockfile: true` and the corresponding packages are removed [pnpm/pnpm#13892](https://github.com/pnpm/pnpm/issues/13892).

- `pnpm update <name>@<version>` now fails with `ERR_PNPM_UPDATE_VERSION_ON_INDIRECT_DEP` when the package is not a direct dependency of any selected project, instead of quietly updating it to whatever a fresh install would resolve. There is nowhere to record the version in that case, so the request cannot be honored, and the error points at the `overrides` entry that does pin a transitive dependency. Ranges and tags are unaffected, and a package that any selected project declares directly still takes its version as before.

- `pnpm update <pkg>@<version>` now updates only the selected packages and leaves unrelated dependencies unchanged. A selector that renames the package it installs — `pnpm update <alias>@npm:<pkg>@<version>` or the `jsr:` equivalent — now targets the package the alias installs rather than the alias.

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.0
  - @pnpm/building.policy@1100.0.20
  - @pnpm/catalogs.config@1100.0.6
  - @pnpm/cli.utils@1101.0.24
  - @pnpm/config.pick-registry-for-package@1101.0.0
  - @pnpm/config.reader@1102.0.0
  - @pnpm/config.version-policy@1100.2.1
  - @pnpm/config.writer@1100.0.23
  - @pnpm/constants@1102.0.0
  - @pnpm/core-loggers@1100.3.3
  - @pnpm/deps.github-actions@1100.1.6
  - @pnpm/deps.inspection.outdated@1100.1.26
  - @pnpm/deps.path@1101.0.0
  - @pnpm/deps.security.signatures@1102.0.0
  - @pnpm/deps.status@1100.1.18
  - @pnpm/error@1100.1.3
  - @pnpm/global.commands@1101.0.0
  - @pnpm/global.packages@1101.0.0
  - @pnpm/hooks.pnpmfile@1100.0.28
  - @pnpm/installing.context@1101.0.0
  - @pnpm/installing.dedupe.check@1100.1.10
  - @pnpm/installing.deps-installer@1104.0.0
  - @pnpm/installing.env-installer@1103.0.0
  - @pnpm/lockfile.fs@1100.2.3
  - @pnpm/lockfile.types@1100.0.20
  - @pnpm/lockfile.utils@1102.0.0
  - @pnpm/network.auth-header@1101.1.11
  - @pnpm/network.fetch@1100.1.13
  - @pnpm/pkg-manifest.reader@1100.0.17
  - @pnpm/pkg-manifest.utils@1100.4.1
  - @pnpm/resolving.npm-resolver@1104.0.0
  - @pnpm/resolving.resolver-base@1101.1.1
  - @pnpm/store.connection-manager@1101.0.0
  - @pnpm/store.controller@1102.0.13
  - @pnpm/types@1102.0.0
  - @pnpm/workspace.project-manifest-reader@1100.0.25
  - @pnpm/workspace.project-manifest-writer@1100.0.15
  - @pnpm/workspace.projects-filter@1100.0.38
  - @pnpm/workspace.projects-graph@1100.0.34
  - @pnpm/workspace.projects-reader@1101.0.24
  - @pnpm/workspace.projects-sorter@1100.0.15
  - @pnpm/workspace.root-finder@1100.0.7
  - @pnpm/workspace.state@1100.0.39
  - @pnpm/workspace.workspace-manifest-reader@1100.1.7
  - @pnpm/workspace.workspace-manifest-writer@1100.1.1
