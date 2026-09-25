## 12.0.0-alpha.17

### Minor Changes

- Added support for alias-less Git dependency adds, preserved locked Git commits during unrelated dependency changes, and reported Git package versions in install logs.

- `pnpm list` and `pnpm why` are now feature complete and behaviorally identical to the TypeScript CLI. `pnpm list` gained `--only-projects`, `--find-by` (finders declared in `.pnpmfile.cjs`), search by version range (`pnpm ls "foo@^2"`), subtree deduplication with `[deduped]` markers, peer/skipped annotations, the package-count summary, `--long` manifest details, resolved tarball URLs and absolute paths in `--json`/`--parseable` output, and `--depth` support for globally installed packages. `pnpm why` gained `--json`, `--parseable`, `--long`, `--prod`/`--dev`/`--no-optional`, `--find-by`, workspace project names in the reverse tree, dependency-field annotations, `[circular]`/`[deduped]` markers, peer-variant hashes, and the `Found N versions` summary.

- Added support for the `cleanupUnusedCatalogs` setting: when enabled, `pnpm add`, `pnpm update`, and `pnpm remove` drop catalog entries from `pnpm-workspace.yaml` that no workspace project references.

- The `enableModulesDir: false` setting is now honored: the install resolves and writes `pnpm-lock.yaml` but creates no `node_modules` directory (unless the global virtual store is enabled, in which case packages are still materialized into the store).

- Command shims now set `NODE_PATH` the way pnpm does: under the isolated `nodeLinker` with a hoist pattern, each shim lists the target package's own `node_modules` directories followed by the hidden hoisted modules directory (`node_modules/.pnpm/node_modules`). The new `extendNodePath: false` setting turns this off.

- Added the `--force` flag to `pnpm install` and `pnpm add`: optional dependencies whose `cpu` / `os` / `libc` / `engines` don't match the host are installed instead of skipped, and a forced install relinks packages that an earlier install already materialized [#13142](https://github.com/pnpm/pnpm/issues/13142).

- `sharedWorkspaceLockfile: false` is now supported by the install family [#12042](https://github.com/pnpm/pnpm/issues/12042): a workspace install runs one dedicated install per project, each with its own `pnpm-lock.yaml`, `node_modules`, and virtual store (a custom `virtualStoreDir` resolves per project), and `pnpm add` / `update` / `remove` in a project operate on that project's own lockfile. Recursive and filtered install-family commands still require a shared lockfile.

- Added PnP install materialization and fixed recovery from expired module caches and broken private lockfiles.

### Patch Changes

- Fixed recovery from interrupted dependency builds in the global virtual store, and made `pnpm fetch` populate the virtual store without linking dependencies into projects.

- Fixed workspace lifecycle ordering and bin linking across isolated and hoisted installs.

- Auto-installed peer dependencies wanted by multiple packages under distinct but compatible ranges now resolve through the ranges' semver intersection (`2` + `^2.2.0` install one provider matching `>=2.2.0 <3.0.0-0`), matching pnpm. Previously such peers were only auto-installed when every consumer declared the identical range or `autoInstallPeersFromHighestMatch` was enabled.

- `engineStrict` now fails the install when an incompatible package is reached through a regular dependency edge of an installable package, even if the package is also optionally reachable — matching pnpm. Packages reachable only through optional edges or skipped parents are still skipped [#13143](https://github.com/pnpm/pnpm/issues/13143).

- Engine checks (`engines.node` / `engines.pnpm`) now match npm-semver's `includePrerelease` semantics exactly: a prerelease version no longer satisfies a fully specified `>=` bound (`9.0.0-alpha.1` does not satisfy `>=9.0.0`), while still satisfying expanded ranges like `9`, `>=9`, and `^9.0.0`.

- Fixed a rare hang where `pnpm install` or `pnpm add` could wait forever: when two tasks fetched the same tarball concurrently, the waiting task could miss the downloader's completion notification and never wake up.
