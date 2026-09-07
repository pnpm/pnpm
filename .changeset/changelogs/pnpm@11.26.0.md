## 11.26.0

### Minor Changes

- Catalogs can now resolve workspace dependencies through the `workspace:` protocol.

- `pnpm remove` and `pnpm update` now accept `--trust-lockfile`, `--no-trust-lockfile`, `--trust-policy`, `--trust-policy-exclude`, and `--trust-policy-ignore-after`. `pnpm remove` checks the whole lockfile against the active policies unless `--trust-lockfile` is set.

- Added `pnpm change check` for CI validation of package versions against the `versioning.epics` bands and `versioning.fixed` groups in `pnpm-workspace.yaml`.

### Patch Changes

- Fetch and tarball errors and retry logs now hide URL credentials, query strings, and fragments that could expose secrets.

- Fixed a race during config dependency updates that could redirect a lockfile write through a symlink [#14322](https://github.com/pnpm/pnpm/issues/14322).

- `pnpm add --allow-build=!<pkg>` now correctly denies builds, including in global installs. `pnpm approve-builds <pkg>` and `pnpm approve-builds !<pkg>` now save decisions even when the package is not awaiting approval, with a warning [#14067](https://github.com/pnpm/pnpm/issues/14067).

- Fixed `pnpm audit --fix` failing without a value or when followed by another flag. `pnpm audit --fix=override` now respects `saveExact` and `savePrefix` when writing overrides [#13261](https://github.com/pnpm/pnpm/issues/13261), [#11523](https://github.com/pnpm/pnpm/issues/11523).

- `pnpm audit` now excludes ignored advisories from vulnerability totals and severity counts, and reports them separately [#14535](https://github.com/pnpm/pnpm/issues/14535).

- `pnpm deploy` no longer requires `injectWorkspacePackages`. If a workspace dependency's peer has multiple possible versions, deployment reports `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` with the conflicting versions. Pin the peer with `overrides` to deploy without injection [#9386](https://github.com/pnpm/pnpm/issues/9386).

- Fixed concurrent installs sharing a store occasionally failing with an `ENOENT` error while importing a package file [#14353](https://github.com/pnpm/pnpm/issues/14353).

- Fixed installation failures when a linked local dependency provides a peer dependency also provided by an ancestor, including with `pnpm deploy --legacy`.

- `pnpm install --node-linker=hoisted` no longer downloads skipped optional dependencies when `node_modules` already exists [#14139](https://github.com/pnpm/pnpm/issues/14139).

- Fixed `pnpm install` rejecting a symlinked lockfile when config dependencies are unchanged. Updates to config dependencies also preserve lockfiles with a byte order mark. Writes through symlinked lockfiles remain blocked [#14372](https://github.com/pnpm/pnpm/issues/14372).

- `pnpm install` now relinks workspace packages when `publishConfig.linkDirectory` changes. Frozen installs require the lockfile to be regenerated [#14488](https://github.com/pnpm/pnpm/issues/14488).

- Auto-installed optional peers now satisfy their declared range even when the workspace root uses a version outside that range [#13867](https://github.com/pnpm/pnpm/issues/13867).

- Fixed global virtual store paths for dependency cycles to consistently account for the runtime engine when dependencies have allowed builds [#14341](https://github.com/pnpm/pnpm/issues/14341).

- Standalone installations now preserve the bundled `node-gyp` files needed to build native dependencies.

- Downloaded runtimes are now available to dependency lifecycle scripts during installation.

- Node.js downloads from `nodeDownloadMirrors` now use URL-scoped npm credentials, including bearer tokens, basic auth, and `tokenHelper` [#14334](https://github.com/pnpm/pnpm/issues/14334).

- Fixed `globalDir` and `globalBinDir` handling in global configuration and environment variables, including `~/` expansion. This fixes `pnpm add -g` failing after `pnpm config set -g global-bin-dir` [#14336](https://github.com/pnpm/pnpm/issues/14336).

- The JavaScript pnpm can again switch to the project's pinned pnpm version on hosts without a matching native binary. If the requested version requires an unavailable native binary, the error now identifies the unsupported host [#13622](https://github.com/pnpm/pnpm/issues/13622).

- Global `pnpm config` commands now skip project package manager version switching, allowing authentication to be configured before downloading the pinned version [#14463](https://github.com/pnpm/pnpm/issues/14463).

- `pnpm self-update`, `pnpm with`, and automatic version switching no longer wait through registry retries when a configured registry has no signatures and `registry.npmjs.org` is unavailable [#14483](https://github.com/pnpm/pnpm/issues/14483).

- Fixed argument forwarding on Windows with `shellEmulator` enabled. Trailing backslashes, line breaks, and literal shell expressions are preserved [#14548](https://github.com/pnpm/pnpm/issues/14548).

- Relative `scriptShell` paths now resolve from the workspace root. Bare command names such as `bash` still use `PATH` [#14422](https://github.com/pnpm/pnpm/issues/14422).

- `pnpm import` now preserves the project-local lockfile when `lockfileDir` points elsewhere and restores the destination lockfile on failure. Branch lockfile imports leave the shared lockfile unchanged [#14563](https://github.com/pnpm/pnpm/issues/14563).

- `catalogMode` and `--save-catalog` no longer move local paths, tarballs, or `workspace:<path>` specifiers into catalogs [#14437](https://github.com/pnpm/pnpm/issues/14437).

- `--side-effects-cache`, `--no-side-effects-cache`, and `PNPM_CONFIG_SIDE_EFFECTS_CACHE` now toggle only the local cache, preserving any remote cache configured in `sideEffectsCache`.

- `pnpm unpublish` now handles registry two-factor authentication challenges through web authentication or a one-time password prompt [#14464](https://github.com/pnpm/pnpm/issues/14464).

- `pnpm outdated` and `pnpm update` now follow GitHub Actions references using self-repository syntax, such as `uses: $/.github/actions/setup`.

- `pnpm remove` now accepts `--unsafe-perm`.
