## 11.28.11

pnpm 11.28.11 fixes frozen installs with `excludeLinksFromLockfile`, `pnpm link` in workspaces, `pnpm deploy` with dangling symlinks, and `verifyDepsBeforeRun` starting needless installs.

### Patch Changes

#### Installing packages

- `pnpm deploy` now keeps a symlink in the project whose target is missing from the source, such as a link into a `node_modules` directory that was removed after the build. Deploy used to fail with `ENOENT` on such a link [#17571](https://github.com/pnpm/pnpm/issues/17571).

- A package with `bundledDependencies` and a build script now keeps its bundled dependencies when it is restored from the side-effects cache [#17523](https://github.com/pnpm/pnpm/issues/17523).

- `pnpm install` now links the runtime that an earlier `pnpm install --no-runtime` skipped [#17542](https://github.com/pnpm/pnpm/issues/17542).

- Commands of a package whose `bin` paths contain excess parent directory segments, such as `../dist/cli.js`, are no longer missing. Such paths now resolve inside the package [#17586](https://github.com/pnpm/pnpm/issues/17586).

- Generated POSIX shell shims now quote their paths, so shell special characters in a path stay literal.

- A repeat `pnpm install` now rewrites the bin shims after `extendNodePath` changes. Before, it reported "Already up to date" and the shims kept the `NODE_PATH` of the previous setting.

#### Lockfile and resolution

- `pnpm install --frozen-lockfile` now accepts the lockfile it wrote with `excludeLinksFromLockfile` and `linkWorkspacePackages` on, when a project depends on a workspace package through a plain version range or a `catalog:` entry. It used to fail with `ERR_PNPM_OUTDATED_LOCKFILE`.

- `pnpm install --frozen-lockfile`, `pnpm approve-builds`, and `pnpm rebuild` now accept a lockfile that resolves a `*` dependency to a prerelease `latest` version. pnpm wrote such a lockfile and then reported it as outdated [#17594](https://github.com/pnpm/pnpm/issues/17594).

- Packages from the default registry keep plain lockfile keys, even when a prefix such as `npmjs` points at the registry set as `registry`. A dependency written with such a prefix shares the plain entry of the same package. The next install rewrites keys that earlier versions qualified this way [#17575](https://github.com/pnpm/pnpm/issues/17575).

- Relative local paths in root `overrides` now resolve from the workspace root when each project has its own lockfile [#17691](https://github.com/pnpm/pnpm/issues/17691).

  Installing a local directory dependency no longer fails when another project's install is writing a lockfile in that directory.

- Peer deduplication no longer gives a project different copies of a dependency across direct and transitive paths when an optional peer has no provider [#17666](https://github.com/pnpm/pnpm/issues/17666).

- A Node.js version range such as `>=26.13`, for example in `devEngines.runtime`, now resolves to the newest stable release that satisfies it. pnpm picks a prerelease only when no stable release matches [#17516](https://github.com/pnpm/pnpm/issues/17516).

#### verifyDepsBeforeRun

- With `verifyDepsBeforeRun`, `pnpm run` and `pnpm exec` now report dependencies as out of date after a dependency is added to the `package.json` of a local directory dependency, such as `"lib": "file:../lib"`. Before, the command ran without the new dependency installed.

- `verifyDepsBeforeRun` no longer reports dependencies as out of sync when only the `version` field of a single-project repository changed [#17560](https://github.com/pnpm/pnpm/issues/17560). The same holds for a workspace project that no dependency can resolve to. A script run after such a change no longer starts an install first.

#### pnpm link

- In a workspace, `pnpm link <dir>` now adds the `link:` dependency to the `package.json` of the project where the command runs, not to the workspace root's `package.json`. The link is created in that project's `node_modules`, as documented. The override is still written to the root `pnpm-workspace.yaml`.

- `pnpm link` with several paths now records every linked package in the `overrides` of `pnpm-workspace.yaml`. Some of them could be missing before.

- `pnpm link` now fails without changing `package.json` or `pnpm-workspace.yaml` when the linked package's name is not a valid npm package name. It used to write that name to both files, which made every later install fail [#17475](https://github.com/pnpm/pnpm/issues/17475).

#### pnpm update

- `pnpm update` with only negated selectors, such as `pnpm update '!foo'`, now leaves the dependencies listed in `update.ignoreDeps` unchanged [pnpm/tasks#243](https://github.com/pnpm/tasks/issues/243).

- `pnpm update --depth 0` without selectors now does nothing when every direct dependency is listed in `update.ignoreDeps`. Before, it failed with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` [pnpm/tasks#251](https://github.com/pnpm/tasks/issues/251).

#### Store and self-update

- `pnpm store prune` now keeps packages linked from a configured modules directory [pnpm/tasks#230](https://github.com/pnpm/tasks/issues/230).

- `pnpm store status` no longer reports untouched packages as modified when the global virtual store is enabled. If the installed global-store layout cannot be verified, the command reports `ERR_PNPM_STORE_STATUS_UNVERIFIABLE` and asks for a reinstall [#17631](https://github.com/pnpm/pnpm/issues/17631).

- `pnpm store add` now fails right away with `ERR_PNPM_FROZEN_STORE_WRITE` when `frozenStore` is enabled. It used to retry each download for minutes before failing, or fail with a SQLite error on an empty store.

- `pnpm self-update` now reinstalls a cached engine if its executable cannot run. This repairs shared store entries damaged by an earlier install [#17582](https://github.com/pnpm/pnpm/issues/17582).
