## 1102.3.0

### Minor Changes

- Added the `forceIgnoresPlatform` setting. When it is `false`, `pnpm install --force` skips optional dependencies whose `os`, `cpu` or `libc` do not match the host instead of installing all of them. The default stays `true` [#6133](https://github.com/pnpm/pnpm/issues/6133).

### Patch Changes

- Warn when `shared-workspace-lockfile` is passed on the command line outside a workspace.

- `pnpm install` and `pnpm add` no longer skip optional dependencies that the Node.js version resolved for a `devEngines.runtime` range supports, when the range uses `onFail: download`. An explicitly set `nodeVersion` still takes priority [#14628](https://github.com/pnpm/pnpm/issues/14628).

- `pnpm install` now uses the running Node.js when `devEngines.runtime` declares a range without `onFail: download`. Optional dependencies supported by the active Node.js are no longer skipped [pnpm/pnpm#15230](https://github.com/pnpm/pnpm/issues/15230).

- Fixed command lookup for custom `modulesDir` settings in `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. Project `.hooks` scripts are read from the configured modules directory. Installs resolved by pnpr now preserve configured modules and executable directories. `pnpm bin` now reports the configured executable directory. In a workspace whose projects keep their own lockfiles, a `packageConfigs` entry that gives one project its own `modulesDir` is followed too [#3604](https://github.com/pnpm/pnpm/issues/3604).

- `.npmrc` files now support npm's `${VAR?}` placeholder. It expands to the value of `VAR`, or to an empty string without a warning when `VAR` is unset [#14404](https://github.com/pnpm/pnpm/issues/14404).

- pnpm now keeps the configured default registry when `_auth` holds credentials for several registries and some of those registries serve package scopes.

  Lockfile verification checks a tarball hosted on a scoped registry against that registry's metadata, unless the package's own scope has a registry assigned [pnpm/pnpm#15530](https://github.com/pnpm/pnpm/issues/15530).

- Warn when an empty environment variable removes an `.npmrc` authentication token. Authentication environment warnings now name the affected key [pnpm/pnpm#4806](https://github.com/pnpm/pnpm/issues/4806).

- pnpm no longer treats packages inside a custom `modulesDir` as workspace projects, including one that `packageConfigs` sets for a project. Before, with a `modulesDir` such as `vendor` and a `packages` pattern such as `**`, a repeat install ran the lifecycle scripts of dependencies that `allowBuilds` had not approved [#15412](https://github.com/pnpm/pnpm/pull/15412).

- `CreateNewStoreControllerOptions` is now exported from `@pnpm/store.connection-manager`. Store controllers now read custom certificate authorities from `cafile`.

- Expand environment variables in `_auth.authToken` values loaded from global `config.yaml` and `pnpm_config__auth`.

- pnpm no longer expands environment variables in a `userAgent` set in a project's `pnpm-workspace.yaml`. A `userAgent` with a placeholder in that file is now ignored. Before this fix, pnpm sent the variable's value to the configured registry [#15415](https://github.com/pnpm/pnpm/issues/15415).

- Updated dependencies:
  - @pnpm/catalogs.config@1100.0.8
  - @pnpm/error@1100.2.0
  - @pnpm/hooks.pnpmfile@1100.0.33
  - @pnpm/network.git-utils@1100.0.5
  - @pnpm/pkg-manifest.utils@1100.4.6
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
  - @pnpm/workspace.workspace-manifest-reader@1100.2.0
