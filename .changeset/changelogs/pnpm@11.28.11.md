## 11.28.11

### Patch Changes

- `pnpm install` now links the runtime that an earlier `pnpm install --no-runtime` skipped [pnpm/pnpm#17542](https://github.com/pnpm/pnpm/issues/17542).

- `pnpm deploy` now keeps a symlink in the project whose target is missing from the source, such as a link into a `node_modules` directory that was removed after the build. Deploy used to fail with `ENOENT` on such a link [#17571](https://github.com/pnpm/pnpm/issues/17571).

- Packages from the default registry keep plain lockfile keys, even when a prefix such as `npmjs` points at the registry set as `registry`. A dependency written with such a prefix shares the plain entry of the same package. The next install rewrites keys that earlier versions qualified this way [#17575](https://github.com/pnpm/pnpm/issues/17575).

- `pnpm install --frozen-lockfile`, `pnpm approve-builds`, and `pnpm rebuild` now accept a lockfile that resolves a `*` dependency to a prerelease `latest` version. pnpm wrote such a lockfile and then reported it as outdated [#17594](https://github.com/pnpm/pnpm/issues/17594).

- Relative local paths in root `overrides` now resolve from the workspace root when each project has its own lockfile [#17691](https://github.com/pnpm/pnpm/issues/17691).

  Installing a local directory dependency no longer fails when another project's install is writing a lockfile in that directory.

- `pnpm store prune` now keeps packages linked from a configured modules directory [pnpm/tasks#230](https://github.com/pnpm/tasks/issues/230).

- A package with `bundledDependencies` and a build script now keeps its bundled dependencies when it is restored from the side-effects cache [pnpm/pnpm#17523](https://github.com/pnpm/pnpm/issues/17523).

- Fixed peer deduplication giving a project different copies of a dependency across direct and transitive paths when an optional peer has no provider [pnpm/pnpm#17666](https://github.com/pnpm/pnpm/issues/17666).

- Fixed missing commands when a package's `bin` paths contain excess parent directory segments. Paths such as `../dist/cli.js` now resolve inside the package [pnpm/pnpm#17586](https://github.com/pnpm/pnpm/issues/17586).

- `pnpm install --frozen-lockfile` now accepts a lockfile written with `excludeLinksFromLockfile` and `linkWorkspacePackages` on, when a project depends on a workspace package through a `catalog:` entry. It used to fail with `ERR_PNPM_OUTDATED_LOCKFILE` once `node_modules` was removed.

- `pnpm install --frozen-lockfile` now accepts the lockfile it wrote with `excludeLinksFromLockfile` and `linkWorkspacePackages` on, when a project depends on a workspace package through a plain version range. It used to fail with `ERR_PNPM_OUTDATED_LOCKFILE`. The frozen install now links the workspace package again.

- In a workspace, `pnpm link <dir>` now adds the `link:` dependency to the `package.json` of the project where the command runs, not to the workspace root's `package.json`. The link is created in that project's `node_modules`, as documented. The override is still written to the root `pnpm-workspace.yaml`.

- `pnpm link` with several paths now records every linked package in the `overrides` of `pnpm-workspace.yaml`. Some of them could be missing before.

- `pnpm link` now fails without changing `package.json` or `pnpm-workspace.yaml` when the linked package's name is not a valid npm package name. It used to write that name to both files, which made every later install fail [#17475](https://github.com/pnpm/pnpm/issues/17475).

- `verifyDepsBeforeRun` no longer reports dependencies as out of sync when only the `version` field of a single-project repository changed. A script run after such a change no longer starts an install first [#17560](https://github.com/pnpm/pnpm/issues/17560).

- `pnpm update` with only negated selectors, such as `pnpm update '!foo'`, now leaves the dependencies listed in `update.ignoreDeps` unchanged [pnpm/tasks#243](https://github.com/pnpm/tasks/issues/243).

- A Node.js version range such as `>=26.13`, for example in `devEngines.runtime`, now resolves to the newest stable release that satisfies it. pnpm picks a prerelease only when no stable release matches [pnpm/pnpm#17516](https://github.com/pnpm/pnpm/issues/17516).

- `pnpm self-update` now reinstalls a cached engine if its executable cannot run. This repairs shared store entries damaged by an earlier install [pnpm/pnpm#17582](https://github.com/pnpm/pnpm/issues/17582).

- A repeat `pnpm install` now rewrites the bin shims after `extendNodePath` changes. Before, it reported "Already up to date" and the shims kept the `NODE_PATH` of the previous setting.

- Quote paths in generated POSIX shell shims so shell special characters stay literal.

- `pnpm store add` now fails right away with `ERR_PNPM_FROZEN_STORE_WRITE` when `frozenStore` is enabled. It used to retry each download for minutes before failing, or fail with a SQLite error on an empty store.

- Fixed `pnpm store status` reporting untouched packages as modified when the global virtual store is enabled. If the installed global-store layout cannot be verified, the command reports `ERR_PNPM_STORE_STATUS_UNVERIFIABLE` and asks for a reinstall. [pnpm/pnpm#17631](https://github.com/pnpm/pnpm/issues/17631).

- `pnpm update --depth 0` without selectors now does nothing when every direct dependency is listed in `update.ignoreDeps`. Before, it failed with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` [pnpm/tasks#251](https://github.com/pnpm/tasks/issues/251).

- With `verifyDepsBeforeRun`, `pnpm run` and `pnpm exec` now report dependencies as out of date after a dependency is added to the `package.json` of a local directory dependency, such as `"lib": "file:../lib"`. Before, the command ran without the new dependency installed.

- `verifyDepsBeforeRun` no longer reports dependencies as out of sync when only the `version` field of a workspace project changed and no dependency can resolve to that project. A script run after such a change no longer starts an install first.
