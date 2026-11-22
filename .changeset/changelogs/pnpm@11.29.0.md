## 11.29.0

pnpm 11.29.0 stops reading `otp` from config files, rejects local filesystem dependencies declared by registry packages, adds the `--minimum-release-age` option, and recreates an incompatible `node_modules` without a TTY. This release carries several security fixes.

### Minor Changes

- `otp` is no longer read from any config file. Pass `--otp` on the command line, or set `PNPM_CONFIG_OTP`. A `pnpm-workspace.yaml` or a global `config.yaml` that carries `otp` is ignored, with a warning naming those two channels. `pnpm config set otp` fails with `ERR_PNPM_CONFIG_SET_NOT_A_FILE_SETTING` instead of writing an entry no loader reads back.

  This closes a way to leak a secret. A repository could write `otp: ${NPM_TOKEN}` beside its own `registry:`, and `pnpm publish` sent that environment variable to the registry the same file named, in the `npm-otp` header of its first request [#13542](https://github.com/pnpm/pnpm/issues/13542).

### Patch Changes

#### Security

- `pnpm install` now rejects local filesystem dependencies (`file:`, `link:`, and path-based `workspace:` specifiers) declared by non-local packages with `ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED`. Relative `file:` dependencies inside a package's own archive and local dependencies configured by the workspace through `overrides`, `packageExtensions`, or `.pnpmfile.cjs` continue to work. Lockfile verification also rejects unauthorized local directory, tarball, and link entries on non-importer snapshots.

- `pnpm install` and `pnpm install --frozen-lockfile` now enforce `blockExoticSubdeps` against entries in `pnpm-lock.yaml`. Before, pnpm accepted a lockfile with git or tarball URL subdependencies and installed those packages even with `blockExoticSubdeps: true`.

- Tarball downloads send registry credentials only over HTTPS or loopback HTTP. Tarball and package metadata requests respect registry credential paths on redirects. A failed tarball request can no longer pin its redirect destination in the lockfile.

- Tarball URL preflight requests use the configured credentials and fetch retry settings.

- TLS file paths in `.npmrc` and `auth.ini` resolve relative to the file that declares them. Project `.npmrc` files skip TLS file paths containing environment variables.

  `pnpm config get` refuses to print protected auth settings. `pnpm config list` hides TLS private keys and inline proxy credentials. `pnpm config set --global tokenHelper` writes to `auth.ini`. Setting a project-level `tokenHelper` fails.

- Global commands (`--global`, `-g`) no longer load `.pnpmfile.cjs`, `pnpm-workspace.yaml`, or `.npmrc` from the current working directory. `pnpm why -g` now inspects globally installed packages.

- `pnpm dlx`, `pnx`, and `pnpm create` now handle local isolation, script skipping, and non-registry package specifiers consistently:

  - `pnpm` ignores local `.pnpmfile.cjs` hooks and project `.npmrc` proxy or TLS settings in the working directory.
  - Running with `--ignore-scripts` or `ignoreScripts: true` uses a separate `dlx` cache key, keeping cached installs that skipped build scripts separate from normal runs.
  - Relative local directory and tarball specifiers such as `./my-cli` and `./my-cli.tgz` resolve against the caller's working directory.
  - Explicitly requested tarball URL, `file:`, and git URL packages run their build scripts by default unless `--ignore-scripts` or a negated `--allow-build` selector disables them.

- `pnpm approve-builds` now requires the exact selector shown by `pnpm ignored-builds` for a dependency a bare package name cannot identify (a `file:`, git, or tarball artifact). A bare package name only approves a plain registry version [#18237](https://github.com/pnpm/pnpm/issues/18237).

- `pnpm pack` and `pnpm publish` now reject `publishConfig.directory` values that use an absolute path or `..` segments outside the package directory.

- `pnpm install` with `pnprServer` now rejects registry-keyed lockfile entries whose resolution uses a non-registry source with `ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`. The `pnpr` lockfile verification and resolution endpoints enforce the same check before accepting or caching a lockfile.

- `pnpm install` with `pnprServer` configured now falls back to local resolution when the project declares `file:`, `link:`, or local path dependencies. `pnpr` also rejects local filesystem dependency specifiers on `/-/pnpr/v0/resolve`.

- Installs through a pnpr server now enforce `minimumReleaseAgeStrict` and add fallback versions to `minimumReleaseAgeExclude` when strict mode is off.

- `pnpm install` now records the supply-chain verification of the lockfile it writes by the lockfile's content. Before, a lockfile that another process put in its place right after the write could reuse that verification on the next install.

- Matching a glob that repeats a wildcard, such as `+(*)z` in the workspace `packages` patterns, no longer hangs on long paths. The bundled picomatch is patched for a [security advisory](https://github.com/advisories/GHSA-fc5q-39xc-vjpf).

#### Installing packages

- `pnpm install` no longer fails without a TTY when it has to recreate an incompatible `node_modules` directory, such as in a Docker build or with piped stdin [#6778](https://github.com/pnpm/pnpm/issues/6778), [#9166](https://github.com/pnpm/pnpm/issues/9166), [#8654](https://github.com/pnpm/pnpm/issues/8654), [#8085](https://github.com/pnpm/pnpm/issues/8085). It now recreates the directory and prints a warning that explains why. A custom or symlinked modules directory still requires setting `confirmModulesPurge` to `false`.

- `pnpm install` now installs a workspace project located in a parent directory of the workspace root, such as one matched by `packages: ['..']` [#10096](https://github.com/pnpm/pnpm/issues/10096).

- Fixed `ERR_PNPM_LINKING_FAILED` on filesystems without copy-on-write reflinks when an install reuses an existing global virtual store entry before staging a package build. Also fixed merging `node_modules` directories during staged package imports on filesystems such as Btrfs that report `EEXIST` when renaming onto a non-empty directory.

- A filtered `pnpm install` that changes the installed dependency groups, such as `pnpm --filter app install --prod`, no longer empties the `node_modules` directories of the workspace projects it skips.

- With `enableGlobalVirtualStore` on, `require.extensions` hooks run again for `require()` calls from a module compiled from a string, such as the module Next.js uses to load `next.config.ts`. A required `.ts` file skipped the registered TypeScript hook. A required `.json` file was parsed as JavaScript [#18261](https://github.com/pnpm/pnpm/issues/18261).

- Fixed `nodeLinker: hoisted` installs omitting a dependency that its parent also declares as a peer dependency when `autoInstallPeers` is disabled [#18193](https://github.com/pnpm/pnpm/issues/18193).

- Fixed `nodeLinker: hoisted` installs placing a package where one of its dependencies resolves to another version than the lockfile records. It could happen in a workspace whose projects depend on different versions of the same packages. In a workspace where one project's directory is inside another's, pnpm now keeps the inner project's dependency in its own `node_modules` when the outer project holds another version.

- Fixed `pnpm prune` leaving extraneous package links in hoisted installations.

- Fixed Plug'n'Play installs with local `file:` directory dependencies. Root projects can resolve their dependencies, and workspace projects with duplicate names retain their own dependencies.

- Plug'n'Play now combines custom export conditions from `NODE_OPTIONS` and command-line flags. It also respects Node.js's enabled `node-addons` and `module-sync` conditions.

- Fixed package maps resolving workspace and linked packages to the wrong directory when a path contains `#`, `?`, or `%`.

- `pnpm fetch` now applies patches to packages that are only reachable through local `file:` dependencies [#11034](https://github.com/pnpm/pnpm/issues/11034).

- Fixed `pnpm install --lockfile-only` fetching unchanged Git dependencies when an unrelated dependency changes and `trustLockfile` is enabled [#18217](https://github.com/pnpm/pnpm/issues/18217).

- When an overrides entry requests a version that does not exist on the registry, `pnpm install` now names that override entry in the error. If the entry's selector has a version range, such as `foo@^1`, the error also shows the latest published version in that range.

- When an install is delegated to the Rust engine, pnpm now renders the lockfile verification progress that the engine reports. The pass and failure lines show how many entries were checked.

#### Lockfile and frozen installs

- `pnpm install --frozen-lockfile` now accepts a plain-range dependency of a local directory dependency that `linkWorkspacePackages: deep` linked to a workspace project. It fails when the `injected` setting of a dependency of a local directory dependency no longer matches the lockfile [pnpm/tasks#319](https://github.com/pnpm/tasks/issues/319).

- Frozen installs now detect changes to catalog entries used only by peer dependencies of local directory packages.

- `pnpm install --frozen-lockfile` now revalidates registry metadata when verifying a locked tarball whose version is missing from the cached package listing.

- `pnpm ci` no longer reports an existing current lockfile to the `preResolution` hook when `virtualStoreDir` points outside the project and the lockfile is about to be removed with `node_modules`.

- pnpm can read lockfiles with large YAML merge mappings or ordered maps without the excessive parsing time seen in some cases [#18246](https://github.com/pnpm/pnpm/issues/18246).

- `pnpm-lock.yaml` now quotes strings that look like numbers, such as `+.5`, so they are read back as strings.

#### Workspaces and scripts

- The workspace project graph now follows `overrides`. An override that points a dependency at a workspace package, such as `foo: workspace:*`, makes that package a dependency of every project whose declaration the override matches, even with `linkWorkspacePackages` off. `pnpm install` now runs that package's lifecycle scripts before the scripts of the projects depending on it. `pnpm -r run` orders the projects the same way. `--filter` selectors that follow dependencies now see the edge.

- A `[<since>]` filter such as `--filter "...[origin/main]"` now selects a project whose dependency an `overrides` entry points at a changed catalog entry. For example, with `lib: catalog:local` in `overrides`, changing `lib` in the `local` catalog selects every project that depends on `lib`.

- `pnpm install` now runs the workspace root's `pnpm:devPreinstall` script when `sharedWorkspaceLockfile` is `false` [#4503](https://github.com/pnpm/pnpm/issues/4503). `pnpm install --dry-run` no longer runs it.

- `pnpm run` and `pnpm exec` now find the Plug'n'Play loader and experimental package map when `lockfileDir` is configured. This also applies to recursive commands.

- `pnpm run` and `pnpm exec` no longer trigger a reinstall solely because the last install used `--trust-lockfile` [#18218](https://github.com/pnpm/pnpm/issues/18218).

- After a workspace script runs, pnpm links commands from scoped injected packages into the containing modules directory's `.bin`. Commands declared through `directories.bin` are also refreshed.

- pnpm skips unsupported command names in `directories.bin`, including filenames with spaces.

- `--resume-from` starts at the named project when the previous run completed no tasks.

- A `tasks` entry with no value in `pnpm-workspace.yaml`, such as `build:`, now fails with a hint. The hint suggests writing `{}` for a task with no settings, or removing the entry to keep the default `dependsOn: ['^<task>']`.

- `pnpm --version`, `pnpm --help`, and `pnpm help` now succeed when the project pins another package manager.

#### Patched dependencies

- Fixed several bugs in applying patches:

  - Deleting a file whose path contains ` b/` no longer deletes a different file.
  - A hunk without context lines now inserts its lines after the line its header names.
  - A patch that creates a file no longer overwrites an existing file with different content. pnpm reports the patch as failed.

- `pnpm patch-commit` now keeps a file without a final newline unterminated when that file comes last in the patch. Before, the patch dropped the final `\ No newline at end of file` marker, so the patched file gained a trailing newline.

- `pnpm patch-remove` now removes the corresponding edit directory and state entry from `node_modules/.pnpm_patches`.

#### Packing, publishing, and deploying

- `pnpm pack --json` now reports the `package.json` that goes into the tarball, with `publishConfig` and the other publish-time changes applied. Run `pnpm pack --dry-run --json` to preview it without writing a tarball. A dry run still runs `prepack` and `prepare`, unless `--ignore-scripts` is passed.

- Packed workspace dependencies now use the dependency's `publishConfig.name`.

- Fixed `pnpm deploy` for projects that depend on an external directory through `link:`.

- `pnpm deploy` now installs a workspace package from its publish directory only when the package's `package.json` sets `publishConfig.directory` to that directory. A publish directory that only the lockfile records is ignored.

- `pnpm stage` uses the current package's publish registry when listing, viewing, approving, rejecting, or downloading staged versions. To use a one-time password at a non-default registry selected by the package, pass `--registry`.

- `pnpm version -r` updates explicit workspace dependency ranges that a release invalidates. `pnpm version` preserves edits that a `preversion` script makes to other manifest fields.

- When `pnpm unpublish` removes the version tagged `latest`, it prefers the highest remaining stable version.

#### Configuration

- Added the `--minimum-release-age` and `--minimum-release-age-exclude` options to `pnpm install`, `pnpm add`, `pnpm update`, and `pnpm dlx`. They set `minimumReleaseAge` and `minimumReleaseAgeExclude` for a single run without editing a config file [#11224](https://github.com/pnpm/pnpm/issues/11224).

- `minimumReleaseAge` must now be a finite, non-negative number of minutes. Other values fail with `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE`. Before, such a value turned off the release age check or crashed the install.

- Approving the `minimumReleaseAge` prompt no longer reprints the list of packages it just showed. pnpm now confirms with one line that says how many entries it added to `minimumReleaseAgeExclude` [#15377](https://github.com/pnpm/pnpm/issues/15377).

- A setting in `pnpm-workspace.yaml` or the global `config.yaml` that references an undefined environment variable without a fallback now fails with `ERR_PNPM_CONFIG_UNRESOLVED_ENV_VAR`. Before, pnpm failed with an error that had no code [#10963](https://github.com/pnpm/pnpm/issues/10963).

- Empty or whitespace-only numeric `pnpm_config_*` environment variables no longer override configured values or defaults.

- pnpm now validates `noProxy`, `strictSsl`, `localAddress`, and `sideEffectsCache.exclude` in workspace and global configuration. `noProxy` accepts strings and booleans, and `null` leaves it unset.

- pnpm now reports `referenceTime` in `pnpm-workspace.yaml` as a setting of a newer pnpm version.

- Windows command shims preserve existing PATH entries when the `prependToPath` option is used.

#### Other commands and help

- `pnpm store prune` now fails with `ERR_PNPM_FROZEN_STORE_WRITE` when `frozenStore` is enabled. Before, it removed files from the store.

- `pnpm peers check` now resolves the `catalog:` peer ranges of injected workspace packages through the workspace catalogs. With `autoInstallPeers: false`, such a peer was reported as unmet even when the installed version matched the catalog entry [#10279](https://github.com/pnpm/pnpm/issues/10279).

- Fixed `pnpm update` and `pnpm outdated` with `--include-github-actions` corrupting workflow YAML or skipping actions referenced through YAML anchors, aliases, or merge keys. Combining a GitHub Action selector with a negated package selector such as `!lodash` no longer matches every package dependency.

- `pnpm outdated` now reports newer prereleases of an installed prerelease. It checks the dist-tag that points to the newest version of the installed prerelease channel, such as `next` for `3.0.0-rc.0`, alongside `latest` [#7339](https://github.com/pnpm/pnpm/issues/7339).

- `pnpm list -g` with `--depth` greater than `0` now always reads the global install directory's `node_modules` and lockfile. Before, it could read the current project's lockfile or a custom `modulesDir`.

- `pnpm list --json` now reports unsaved dependencies from each workspace project's own modules directory [#18248](https://github.com/pnpm/pnpm/issues/18248).

- `pnpm list --lockfile-only --find-by` now reads manifests from the configured store when `node_modules` is absent.

- `pnpm why --exclude-peers` now leaves out packages that are reached only as a resolved peer dependency. Before, the flag was accepted but ignored.

- `pnpm cat-file` can read executable files from the store. `pnpm cache list`, `view`, and `delete` recognize mixed-case package names.

  `pnpm clean` removes custom virtual stores with hidden directory names. In Plug'n'Play projects it also removes the generated loader on a full clean.

- Fixed `pnpm unlink` in CI and in workspace subprojects. It now removes matching linked dependencies from the selected manifests.

- Fixed `pnpm link` resolving a quoted `~/` path from the current directory.

- `pnpm import` respects the symlink policy for the destination lockfile and skips Yarn patches outside the workspace.

- `pnpm add` run from a workspace package directory that has no `package.json` yet now suggests creating one with `pnpm init` [#11345](https://github.com/pnpm/pnpm/issues/11345).

- Removed the obsolete `xmas` command.

- `pnpm help` and `pnpm <command> --help` now print the help when the system runtime does not satisfy the project's `devEngines.runtime` or `engines.runtime`.

- `pnpm clean --help` and the help of other commands a script can override now print the help, even when `package.json` has a script with the same name.

- An unknown option passed via a shorthand is now reported as typed, and the shorthand of the other case is suggested when the command supports it. For instance, `pnpm update -l` used to fail with `Unknown option: 'long'`, as `-l` is short for `--long`. Now it fails with `Unknown option: '-l'` and suggests `-L` (`--latest`).

- `pnpm help import` now lists the output options, such as `--reporter`, that the command accepts [#7829](https://github.com/pnpm/pnpm/issues/7829).

- The help for `pnpm config get` and `pnpm get` now says that the key is a property path, such as `catalog.react`.

- Tab completion suggests project scripts when you type `pnpm` followed by a script name [#18239](https://github.com/pnpm/pnpm/issues/18239).
