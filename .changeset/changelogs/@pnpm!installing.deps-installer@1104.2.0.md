## 1104.2.0

### Minor Changes

- Added the `forceIgnoresPlatform` setting. When it is `false`, `pnpm install --force` skips optional dependencies whose `os`, `cpu` or `libc` do not match the host instead of installing all of them. The default stays `true` [#6133](https://github.com/pnpm/pnpm/issues/6133).

### Patch Changes

- `pnpm add` now keeps the specifier a `readPackage` hook provides when the hook rewrites the requested one. When the hook removes the dependency instead, the add skips it and reports why. Previously the add wrote the request either way, and the hook undid it on the next read, so `pnpm install --frozen-lockfile` failed [#15156](https://github.com/pnpm/pnpm/issues/15156).

- `pnpm audit --fix=update` now fixes vulnerabilities in dependencies declared through an npm alias. A specifier such as `"foo": "npm:vulnerable-pkg@1.0.0"` moves to the patched version and keeps the alias. Versions pinned with a leading `=` are fixed as well [#15155](https://github.com/pnpm/pnpm/issues/15155).

- `pnpm dedupe` now moves transitive dependencies to the version a `catalog:` dependency pins, as it already did for versions written directly in `package.json`.

- `pnpm install` and `pnpm add` no longer skip optional dependencies that the Node.js version resolved for a `devEngines.runtime` range supports, when the range uses `onFail: download`. An explicitly set `nodeVersion` still takes priority [#14628](https://github.com/pnpm/pnpm/issues/14628).

- `pnpm add` now saves changes to `package.json` before running lifecycle scripts, so a postinstall script failure leaves the added dependency in `package.json` [#8627](https://github.com/pnpm/pnpm/issues/8627).

- `pnpm install --frozen-lockfile` now rejects changed local tarballs, even when the previous archive contents are in the store [pnpm/pnpm#1889](https://github.com/pnpm/pnpm/issues/1889).

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when `pnpm-lock.yaml` lists a workspace project whose directory or manifest file is missing. The install used to report success without installing that project's dependencies [#7667](https://github.com/pnpm/pnpm/issues/7667).

- `pnpm install --frozen-lockfile` now succeeds when an optional dependency was unresolvable and skipped by the install that wrote the lockfile. Previously, frozen installs failed with `ERR_PNPM_OUTDATED_LOCKFILE`. The notice states that the dependency could not be resolved and names the requested range [pnpm/pnpm#3960](https://github.com/pnpm/pnpm/issues/3960).

- `pnpm install --frozen-lockfile` now fails when a workspace package's version no longer satisfies the range that a dependent workspace project declares for it. This includes injected workspace dependencies [#7823](https://github.com/pnpm/pnpm/issues/7823).

- `pnpm install` now applies changes to or removal of a global `readPackage` hook when an existing lockfile is present [pnpm/pnpm#15136](https://github.com/pnpm/pnpm/issues/15136).

- Workspace projects that `hoistPattern` or `publicHoistPattern` selects are now hoisted on every install. A project added to the workspace was not hoisted until `node_modules` was deleted and reinstalled. A workspace that installs nothing from a registry hoisted none of its projects at all [#3642](https://github.com/pnpm/pnpm/issues/3642).

- `pnpm install` with `--filter` now installs only the dependencies of the selected projects when using `nodeLinker: hoisted` [#8882](https://github.com/pnpm/pnpm/issues/8882).

- `pnpm install --ignore-pnpmfile` no longer removes `pnpmfileChecksum` from an up-to-date `pnpm-lock.yaml`. `pnpm install --frozen-lockfile --ignore-pnpmfile` no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when the lockfile records a `pnpmfileChecksum`. A command that resolves dependencies with the pnpmfile ignored still writes the lockfile without it [#10944](https://github.com/pnpm/pnpm/issues/10944).

- Dependencies and executable binaries are now correctly linked and accessible for workspace packages using `publishConfig.directory` and `publishConfig.linkDirectory` [pnpm/pnpm#8338](https://github.com/pnpm/pnpm/issues/8338).

- Fixed command lookup for custom `modulesDir` settings in `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. Project `.hooks` scripts are read from the configured modules directory. Installs resolved by pnpr now preserve configured modules and executable directories. `pnpm bin` now reports the configured executable directory. In a workspace whose projects keep their own lockfiles, a `packageConfigs` entry that gives one project its own `modulesDir` is followed too [#3604](https://github.com/pnpm/pnpm/issues/3604).

- Tools installed in a custom `modulesDir` can load CommonJS plugins installed there, the same way they would from `node_modules`. When executables are symlinks, as with `preferSymlinkedExecutables` or the hoisted linker, this works for `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. A symlinked tool started directly from a shell does not get it. Paths containing the platform path-list separator do not receive this fallback. `extendNodePath: false` disables this fallback [#3604](https://github.com/pnpm/pnpm/issues/3604).

- The `pnpm:peer-dependency-issues` log event, which `--reporter ndjson` prints, no longer lists peers silenced by `peerDependencyRules.ignoreMissing` under `conflicts` or `intersections` [#8295](https://github.com/pnpm/pnpm/issues/8295).

- Installing through a pnpr server now installs a project's peer dependencies when `autoInstallPeers` is enabled. A project that declared only peer dependencies failed with `ERR_PNPM_OUTDATED_LOCKFILE` or skipped its peers [#14833](https://github.com/pnpm/pnpm/issues/14833).

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- `pnpm prune --prod` and production installs now prune excluded development dependencies even when lockfile generation is disabled.

- `pnpm remove` now runs the project's own `preuninstall`, `uninstall`, and `postuninstall` scripts. `preuninstall` and `uninstall` run before dependencies are unlinked. A failure in either stage aborts the removal. `postuninstall` runs after unlinking completes. The `ignoreScripts` setting and `--lockfile-only` skip all three stages [#3276](https://github.com/pnpm/pnpm/issues/3276).

- Removing an entry from `overrides` now re-resolves the packages it targeted. A version the override had locked is no longer kept just because the declared range still accepts it [#4587](https://github.com/pnpm/pnpm/issues/4587).

- The root project's `preinstall` script now runs before dependencies are resolved and linked. A guard such as `npx only-allow yarn` can stop the install before pnpm populates `node_modules` [#3760](https://github.com/pnpm/pnpm/issues/3760).

- Fixed `pnpm install` for workspace projects reached through a symlink, such as a `packages` directory that links to a folder outside the workspace. pnpm now installs their dependencies, and the links in their `node_modules` resolve [#1044](https://github.com/pnpm/pnpm/issues/1044).

- `pnpm install --prod` and other installs that skip `devDependencies` no longer run the `pnpm:devPreinstall` script [#7065](https://github.com/pnpm/pnpm/issues/7065).

- `pnpm update <pkg>` now moves a package off a locked version the registry no longer serves, such as an unpublished release. The lockfile check for supply-chain policies such as `minimumReleaseAge` used to reject that version before the update could replace it [pnpm/pnpm#9953](https://github.com/pnpm/pnpm/issues/9953).

- Added a `--peer` flag to `pnpm update` to update ranges in `peerDependencies` [#8081](https://github.com/pnpm/pnpm/issues/8081).

- `pnpm update --prod` no longer installs devDependencies when run in a project installed with `--prod` [#8038](https://github.com/pnpm/pnpm/issues/8038).

- `pnpm add` now warns when replacing an existing dependency with a specifier pointing to a different source [#14869](https://github.com/pnpm/pnpm/issues/14869).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.33
  - @pnpm/bins.remover@1100.0.25
  - @pnpm/building.after-install@1103.0.6
  - @pnpm/building.during-install@1102.2.3
  - @pnpm/building.policy@1100.1.3
  - @pnpm/catalogs.config@1100.0.8
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/config.package-is-installable@1100.2.0
  - @pnpm/config.parse-overrides@1100.1.6
  - @pnpm/config.version-policy@1100.2.4
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/crypto.hash@1100.0.6
  - @pnpm/deps.graph-hasher@1100.3.4
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/exec.lifecycle@1100.1.19
  - @pnpm/fs.symlink-dependency@1100.0.21
  - @pnpm/hooks.read-package-hook@1100.3.4
  - @pnpm/hooks.types@1101.0.4
  - @pnpm/installing.context@1101.0.6
  - @pnpm/installing.deps-resolver@1102.2.3
  - @pnpm/installing.deps-restorer@1103.2.0
  - @pnpm/installing.linking.direct-dep-linker@1100.0.21
  - @pnpm/installing.linking.hoist@1100.0.33
  - @pnpm/installing.linking.modules-cleaner@1100.1.25
  - @pnpm/installing.modules-yaml@1101.0.4
  - @pnpm/installing.package-requester@1102.2.0
  - @pnpm/lockfile.filtering@1100.2.9
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.preferred-versions@1100.0.35
  - @pnpm/lockfile.pruner@1100.0.25
  - @pnpm/lockfile.settings-checker@1100.2.8
  - @pnpm/lockfile.to-pnp@1101.0.6
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/lockfile.verification@1100.1.7
  - @pnpm/lockfile.walker@1100.0.25
  - @pnpm/network.auth-header@1101.1.14
  - @pnpm/patching.config@1100.1.7
  - @pnpm/pkg-manifest.utils@1100.4.6
  - @pnpm/pnpr.client@3.0.4
  - @pnpm/resolving.local-resolver@1101.2.3
  - @pnpm/resolving.npm-resolver@1104.2.1
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/store.index@1100.3.2
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
