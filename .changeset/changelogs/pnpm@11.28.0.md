## 11.28.0

pnpm 11.28.0 adds the `forceIgnoresPlatform` setting and `pnpm update --peer`, and fixes many bugs in `pnpm deploy`, `--filter`, `nodeLinker: hoisted`, and custom `modulesDir` setups. This release also carries security fixes for shell completion, bin shims on Nix, lifecycle scripts inside a custom `modulesDir`, and `userAgent` placeholders in `pnpm-workspace.yaml`.

### Minor Changes

- Added the `forceIgnoresPlatform` setting. When it is `false`, `pnpm install --force` skips optional dependencies whose `os`, `cpu` or `libc` do not match the host instead of installing all of them. The default stays `true` [#6133](https://github.com/pnpm/pnpm/issues/6133).

### Patch Changes

#### Security

- pnpm no longer expands environment variables in a `userAgent` set in a project's `pnpm-workspace.yaml`. A `userAgent` with a placeholder in that file is now ignored. Before this fix, pnpm sent the variable's value to the configured registry [#15415](https://github.com/pnpm/pnpm/issues/15415).

- pnpm no longer treats packages inside a custom `modulesDir` as workspace projects, including one that `packageConfigs` sets for a project. Before, with a `modulesDir` such as `vendor` and a `packages` pattern such as `**`, a repeat install ran the lifecycle scripts of dependencies that `allowBuilds` had not approved [#15412](https://github.com/pnpm/pnpm/pull/15412).

- On Nix, a dependency's bin named like a system utility such as `sed` can no longer redirect a POSIX bin shim or the `pnpm`, `pn`, `pnpx`, and `pnx` launchers. The shims and launchers now ignore `node_modules` and relative `PATH` entries while they locate their own files. Installing again replaces the shims already in `node_modules` [#14883](https://github.com/pnpm/pnpm/issues/14883).

- Shell completion now omits candidates containing control or invisible formatting characters, and fish completion omits names containing backslashes. Package and script names can no longer inject extra completion records or terminal escape sequences.

- Commands that run pnpm again, such as `pnpm runtime set` and `pnpm env use`, no longer re-run a script that only looks like pnpm. A script named `pnpm` or `pn` that another package installed was run as though it were pnpm.

- `pnpm store prune` now leaves a `dlx` cache root that is a symlink or Windows junction untouched. Cleanup no longer removes directories through that link.

#### Installing packages

- `pnpm install` now fails at once when a registry or tarball server presents a TLS certificate that fails verification, such as a self-signed or expired one. The error names the certificate problem. Such requests were retried for more than a minute [#9134](https://github.com/pnpm/pnpm/issues/9134).

- `pnpm install` no longer appears to hang when a git dependency is fetched over SSH and ssh asks for a key passphrase or a host key confirmation. pnpm now runs ssh in batch mode, so the install fails right away with the ssh error, and a key that needs a passphrase has to be loaded into an SSH agent first. An ssh command selected through `GIT_SSH_COMMAND`, `GIT_SSH`, or the `core.sshCommand` git setting is kept as is [#2227](https://github.com/pnpm/pnpm/issues/2227).

- pnpm no longer crashes on startup when the temporary directory set by `TMPDIR`, `TEMP`, or `TMP` does not exist [#4960](https://github.com/pnpm/pnpm/issues/4960).

- `pnpm install --silent` no longer fails when the install is delegated to pacquet. pnpm also stops passing `-s`, `--loglevel` and the other reporting flags to pacquet [#11936](https://github.com/pnpm/pnpm/issues/11936).

- Fixed `pnpm install` failing with `EEXIST` when a concurrent install cleared the file or directory that was occupying a symlink path. On Windows, a symlink another process is still holding is no longer moved aside and recreated.

- `pnpm install` no longer fails when writing the workspace state file encounters an error. Failures to update the state file now emit a warning instead of aborting the install [#14550](https://github.com/pnpm/pnpm/issues/14550).

- Installing or adding dependencies no longer fails when a previously installed local tarball file was deleted from disk [#8367](https://github.com/pnpm/pnpm/issues/8367).

- `pnpm install` now reads the same local tarball it installs when a dependency's absolute `file:` path contains `..`. Such a path could install a different tarball than the one it read, failing with `ERR_PNPM_TARBALL_INTEGRITY`, or fail to resolve at all.

- `pnpm install --frozen-lockfile` now rejects changed local tarballs, even when the previous archive contents are in the store [#1889](https://github.com/pnpm/pnpm/issues/1889).

- `pnpm add` and `pnpm install` now support installing bzip2 compressed tarballs [#6761](https://github.com/pnpm/pnpm/issues/6761).

- `pnpm install` now fetches committed submodules of git dependencies [#1470](https://github.com/pnpm/pnpm/issues/1470).

- Interrupting `pnpm install` with Ctrl+C or SIGTERM no longer leaves a temporary lockfile (`.pnpm-lock.yaml.*.tmp`) behind in the project [#1418](https://github.com/pnpm/pnpm/issues/1418).

- `pnpm install` and `pnpm run` now reinstall a single project that was moved or renamed together with its `node_modules`. Before, they reported "Already up to date" while links such as Windows junctions still pointed at the old location [#9512](https://github.com/pnpm/pnpm/issues/9512).

- `pnpm install` now relinks a direct dependency whose link in `node_modules` points to a missing target. Before, it reported "Already up to date" and left the broken link [#9758](https://github.com/pnpm/pnpm/issues/9758).

- `pnpm install` and `pnpm add` no longer skip optional dependencies that the Node.js version resolved for a `devEngines.runtime` range supports, when the range uses `onFail: download`. An explicitly set `nodeVersion` still takes priority [#14628](https://github.com/pnpm/pnpm/issues/14628).

- `pnpm install` now uses the running Node.js when `devEngines.runtime` declares a range without `onFail: download`. Optional dependencies supported by the active Node.js are no longer skipped [#15230](https://github.com/pnpm/pnpm/issues/15230).

- `pnpm install --engine-strict` now respects `engines` relaxed by `readPackage` hooks in `.pnpmfile.cjs` [#15482](https://github.com/pnpm/pnpm/issues/15482).

- `pnpm install` now applies changes to or removal of a global `readPackage` hook when an existing lockfile is present [#15136](https://github.com/pnpm/pnpm/issues/15136).

- The project's `.pnpmfile.mjs` or `.pnpmfile.cjs` now runs after the pnpmfiles of config dependency plugins [#9891](https://github.com/pnpm/pnpm/issues/9891).

- A `readPackage` hook that sets a dependency range to a value other than a string, such as `undefined`, now fails the install with an error that names the dependency, the package, and the pnpmfile. Delete the property to remove a dependency [#5517](https://github.com/pnpm/pnpm/issues/5517).

- `pnpm install --prod` and other installs that skip `devDependencies` no longer run the `pnpm:devPreinstall` script [#7065](https://github.com/pnpm/pnpm/issues/7065). They skip `prepare` lifecycle scripts too, as does `pnpm install` given package arguments. `pnpm deploy` does not run the `prepare` scripts of the deployed project [#7282](https://github.com/pnpm/pnpm/issues/7282).

- The root project's `preinstall` script now runs before dependencies are resolved and linked. A guard such as `npx only-allow yarn` can stop the install before pnpm populates `node_modules` [#3760](https://github.com/pnpm/pnpm/issues/3760).

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- `pnpm prune --prod` and production installs now prune excluded development dependencies even when lockfile generation is disabled.

- `pnpm fetch` now also installs the pnpm version that `pnpm-lock.yaml` pins, when it differs from the running pnpm. A later `pnpm install --offline` that switches to the pinned version no longer fails because that version is missing from the store [#11808](https://github.com/pnpm/pnpm/issues/11808).

- A dependency that ships a `binding.gyp` and sets `gypfile: false` no longer gets the `node-gyp rebuild` install script pnpm synthesizes for it. Such a dependency needs no `allowBuilds` entry and is no longer listed under "Ignored build scripts".

- `pnpm install` no longer adds `allowBuilds` placeholder entries to `pnpm-workspace.yaml` when it runs in CI or without a terminal. Interactive installs still add them [#11574](https://github.com/pnpm/pnpm/issues/11574).

- Installing through a pnpr server now installs a project's peer dependencies when `autoInstallPeers` is enabled. A project that declared only peer dependencies failed with `ERR_PNPM_OUTDATED_LOCKFILE` or skipped its peers [#14833](https://github.com/pnpm/pnpm/issues/14833).

#### Resolving and linking dependencies

- pnpm now installs a dependency that a package also declares as an optional peer dependency, for example `lightningcss` in some vite builds. The dependency was missing from `node_modules`, so the package failed to import it [#8912](https://github.com/pnpm/pnpm/issues/8912).

- Removal overrides such as `"parent>peer": "-"` now prevent optional peers from being installed from another workspace package [#15008](https://github.com/pnpm/pnpm/issues/15008).

- Removing an entry from `overrides` now re-resolves the packages it targeted. A version the override had locked is no longer kept just because the declared range still accepts it [#4587](https://github.com/pnpm/pnpm/issues/4587).

- `packageExtensions` and `overrides` entries with a ranged selector (such as `@<X` or `@*`) no longer match a dependency that has no `package.json`, such as a local directory dependency [#15007](https://github.com/pnpm/pnpm/issues/15007).

- Trim leading and trailing whitespace from dependency override selectors in `pnpm.overrides` [#6356](https://github.com/pnpm/pnpm/issues/6356).

- With `trustPolicy: no-downgrade`, pnpm now resolves the newest matching version that is not a trust downgrade. Previously a dependency failed with `ERR_PNPM_TRUST_DOWNGRADE` even when an older version satisfied its range. `pnpm self-update` picks its target version the same way. A request for an exact version still fails [#14176](https://github.com/pnpm/pnpm/issues/14176).

- `pnpm install` and `pnpm update` now resolve a dependency range to the newest matching version that is not deprecated. A version already recorded in the lockfile is still used [#15128](https://github.com/pnpm/pnpm/issues/15128).

- `pnpm add` and `pnpm remove` no longer move unrelated transitive dependencies to other versions. Adding a package and then removing it now leaves `pnpm-lock.yaml` unchanged. Before, the dependencies of auto-installed peers and `npm:` aliased subdependencies could move to a newer version that was already in the lockfile [#11859](https://github.com/pnpm/pnpm/issues/11859).

- `pnpm dedupe` now moves transitive dependencies to the version a `catalog:` dependency pins, as it already did for versions written directly in `package.json`.

- `pnpm install --ignore-pnpmfile` no longer removes `pnpmfileChecksum` from an up-to-date `pnpm-lock.yaml`. `pnpm install --frozen-lockfile --ignore-pnpmfile` no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when the lockfile records a `pnpmfileChecksum`. A command that resolves dependencies with the pnpmfile ignored still writes the lockfile without it [#10944](https://github.com/pnpm/pnpm/issues/10944).

- `pnpm install --frozen-lockfile` now succeeds when an optional dependency was unresolvable and skipped by the install that wrote the lockfile. Previously, frozen installs failed with `ERR_PNPM_OUTDATED_LOCKFILE`. The notice states that the dependency could not be resolved and names the requested range [#3960](https://github.com/pnpm/pnpm/issues/3960).

- pnpm no longer rewrites `packageManagerDependencies` in `pnpm-lock.yaml` when that block pins `@pnpm/exe` beside `pnpm`. The rewrite ran on every command, so `pnpm list` left a clean working tree dirty, and `pnpm version` then refused to run [#14926](https://github.com/pnpm/pnpm/issues/14926).

- `pnpm import` and fresh resolutions now record `integrity` for git-hosted tarballs, such as `codeload.github.com` URLs, even when the tarball is already in the store [#13338](https://github.com/pnpm/pnpm/issues/13338).

- Merging lockfiles now preserves recorded configuration fields such as `overrides`, `neverBuiltDependencies`, `patchedDependencies`, `packageExtensionsChecksum`, `settings`, and `catalogs` [#8366](https://github.com/pnpm/pnpm/issues/8366).

- A lockfile entry whose resolution is unchanged now keeps its recorded `deprecated` message [#5772](https://github.com/pnpm/pnpm/issues/5772).

- pnpm no longer writes a package's legacy array-form `engines`, such as `["node >= 0.8"]`, to the lockfile. It was recorded as an object keyed by index, such as `{'0': node >= 0.8}` [#4518](https://github.com/pnpm/pnpm/issues/4518).

- With `nodeLinker: hoisted`, `hoistWorkspacePackages` now links each workspace project that `hoistPattern` or `publicHoistPattern` selects into the root `node_modules`, unless a hoisted package or a root dependency already uses its name. The project's bins are linked into the root `node_modules/.bin` [#7553](https://github.com/pnpm/pnpm/issues/7553).

- Workspace projects that `hoistPattern` or `publicHoistPattern` selects are now hoisted on every install. A project added to the workspace was not hoisted until `node_modules` was deleted and reinstalled. A workspace that installs nothing from a registry hoisted none of its projects at all [#3642](https://github.com/pnpm/pnpm/issues/3642).

- With `nodeLinker: hoisted`, `pnpm install` now removes the commands of the packages it removes from `node_modules/.bin`, such as a nested copy deduped into the root `node_modules` [#7568](https://github.com/pnpm/pnpm/issues/7568).

- `pnpm install` now links a dependency's bin even when the bin's file does not exist yet, such as a workspace package's bin that a build script creates after install. Previously pnpm printed a `Failed to create bin` warning. The command then stayed missing until `node_modules` was removed [#10007](https://github.com/pnpm/pnpm/issues/10007), [#10216](https://github.com/pnpm/pnpm/issues/10216).

- Executable linking now makes a bin executable for every user. A target with only some execute permission bits set was left non-executable for other users [#3699](https://github.com/pnpm/pnpm/issues/3699). Bin linking leaves workspace and linked dependency files outside `node_modules` unchanged.

- Dependencies and executable binaries are now correctly linked and accessible for workspace packages using `publishConfig.directory` and `publishConfig.linkDirectory` [#8338](https://github.com/pnpm/pnpm/issues/8338).

#### Workspaces and filtering

- Fixed `pnpm install` for workspace projects reached through a symlink, such as a `packages` directory that links to a folder outside the workspace. pnpm now installs their dependencies, and the links in their `node_modules` resolve [#1044](https://github.com/pnpm/pnpm/issues/1044).

- A dependency declared with `catalog:` now counts as a workspace dependency when its catalog entry points at a workspace project, for example `workspace:*` [#15587](https://github.com/pnpm/pnpm/issues/15587). With `linkWorkspacePackages` enabled, so does an `npm:` alias of a workspace project, such as `"math-alias": "npm:math@^1.0.0"`. `pnpm -r run` runs that project first. `--filter <pkg>...` selects it.

- A `workspace:` dependency now resolves to a workspace project whose version is not valid semver, such as `1` or `1.0`. `workspace:*`, `workspace:^`, and `workspace:~` match it. A range identical to the version also matches it [#4567](https://github.com/pnpm/pnpm/issues/4567).

- A `workspace:` dependency with an exact version now resolves to a workspace project whose version carries SemVer build metadata. For example, `workspace:0.5.6-next.3` matches a project at `0.5.6-next.3+f60facc` [#6483](https://github.com/pnpm/pnpm/issues/6483). A workspace project with build metadata is no longer skipped when it matches the requested range and has the same version precedence as the registry package [#2812](https://github.com/pnpm/pnpm/issues/2812).

- Secondary dependencies now prefer the version resolved by the local project's direct dependencies over versions from sibling workspace projects [#7191](https://github.com/pnpm/pnpm/issues/7191).

- `pnpm install` now re-resolves a workspace project's auto-installed peer dependency when another workspace project changes its specifier for that package to one that excludes the locked version but still overlaps the peer range. The peer then resolves to the version a fresh install would pick [#11800](https://github.com/pnpm/pnpm/issues/11800).

- `pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when `pnpm-lock.yaml` lists a workspace project whose directory or manifest file is missing. The install used to report success without installing that project's dependencies [#7667](https://github.com/pnpm/pnpm/issues/7667).

- `pnpm install --frozen-lockfile` now fails when a workspace package's version no longer satisfies the range that a dependent workspace project declares for it. This includes injected workspace dependencies [#7823](https://github.com/pnpm/pnpm/issues/7823).

- `pnpm install -r` now installs every workspace project when `recursiveInstall` is set to `false` in `pnpm-workspace.yaml` [#7504](https://github.com/pnpm/pnpm/issues/7504).

- `pnpm install` with `--filter` now installs only the dependencies of the selected projects when using `nodeLinker: hoisted` [#8882](https://github.com/pnpm/pnpm/issues/8882).

- `pnpm install` now updates an injected workspace dependency after that package's own dependencies change, when `shared-workspace-lockfile` is `false` [#7209](https://github.com/pnpm/pnpm/issues/7209).

- `syncInjectedDepsAfterScripts` now copies files into injected dependencies when `node_modules` is on another filesystem than the package sources. The sync previously failed with a cross-device link error and made the script run exit with an error [#14703](https://github.com/pnpm/pnpm/issues/14703).

- Fixed injected workspace dependency synchronization failing with `EPERM` on Windows when removing nested directories.

- Workspace discovery now returns one project per directory when multiple manifest formats are present. It selects `package.json`, then `package.json5`, then `package.yaml` [#3027](https://github.com/pnpm/pnpm/issues/3027).

- Wildcards in negated `packages` patterns of `pnpm-workspace.yaml` now match directories whose names start with a dot. For example, `!packages/**` now also excludes `packages/.dev/tool` when another pattern includes `.dev` explicitly.

- pnpm now warns when a workspace install covers a project that has its own `pnpm-workspace.yaml`. The nested file's settings, such as `patchedDependencies`, do not apply when the outer workspace installs that project. pnpm reads settings only from the `pnpm-workspace.yaml` at the workspace root [#11724](https://github.com/pnpm/pnpm/issues/11724).

- pnpm now warns when `shared-workspace-lockfile` is passed on the command line outside a workspace [#1617](https://github.com/pnpm/pnpm/issues/1617).

- The `[<since>]` filter selector now compares against the commit where the current branch forked from `<since>`. Projects changed only by newer commits on `<since>` are no longer selected. Uncommitted changes are still included. In a shallow clone without that commit, pnpm compares against `<since>` directly, as before [#9907](https://github.com/pnpm/pnpm/issues/9907).

- `--filter "[<since>]"` now selects workspace packages when dependency versions change in a catalog in `pnpm-workspace.yaml` [#8718](https://github.com/pnpm/pnpm/issues/8718). It also selects projects that files were moved out of when git detects the move as a rename [#15481](https://github.com/pnpm/pnpm/issues/15481).

- `--filter` now evaluates selectors in order, so later inclusion filters can re-include packages that an earlier exclusion filter excluded [#9354](https://github.com/pnpm/pnpm/issues/9354).

- Directory filters such as `--filter=./packages/*` now select projects when the current directory was entered with a lowercase drive letter on Windows, like `c:\repo` [#5500](https://github.com/pnpm/pnpm/issues/5500).

#### Custom modulesDir

- Bin shims, `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install now find commands in a custom `modulesDir` [#3604](https://github.com/pnpm/pnpm/issues/3604). Project `.hooks` scripts are read from the configured modules directory. Installs resolved by pnpr now preserve configured modules and executable directories. `pnpm bin` now reports the configured executable directory. In a workspace whose projects keep their own lockfiles, a `packageConfigs` entry that gives one project its own `modulesDir` is followed too.

- Tools installed in a custom `modulesDir` can load CommonJS plugins installed there, the same way they would from `node_modules`. When executables are symlinks, as with `preferSymlinkedExecutables` or the hoisted linker, this works for `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. A symlinked tool started directly from a shell does not get it. Paths containing the platform path-list separator do not receive this fallback. `extendNodePath: false` disables this fallback [#3604](https://github.com/pnpm/pnpm/issues/3604).

- A repeat `pnpm install` in a workspace with a custom `modulesDir` now takes the up-to-date fast path. Before, pnpm looked for each workspace project's dependencies in `node_modules` and ran a full install every time.

#### Adding, updating, and removing dependencies

- `pnpm add` now keeps the specifier a `readPackage` hook provides when the hook rewrites the requested one. When the hook removes the dependency instead, the add skips it and reports why. Previously the add wrote the request either way, and the hook undid it on the next read, so `pnpm install --frozen-lockfile` failed [#15156](https://github.com/pnpm/pnpm/issues/15156).

- `pnpm add` now saves changes to `package.json` before running lifecycle scripts, so a postinstall script failure leaves the added dependency in `package.json` [#8627](https://github.com/pnpm/pnpm/issues/8627).

- `pnpm add` now saves the requested exact version when adding a dependency, even when the manifest already contains a version range [#6040](https://github.com/pnpm/pnpm/issues/6040).

- `pnpm add` and `pnpm install` keep an empty `peerDependencies`, `dependencies`, `devDependencies`, or `optionalDependencies` field that was already in `package.json`. pnpm still drops such a field when it removes the last entry itself, as `pnpm remove` does [#5096](https://github.com/pnpm/pnpm/issues/5096).

- Added a `--peer` flag to `pnpm update` to update ranges in `peerDependencies` [#8081](https://github.com/pnpm/pnpm/issues/8081).

- `pnpm update` now keeps a version range whose shape has no save prefix, such as `<= 3.0.0` or `>=1.0.0 <2.0.0`, when the updated version still satisfies it. Before, `<= 3.0.0` became `^3.0.0` [#6714](https://github.com/pnpm/pnpm/issues/6714).

- `pnpm update <name>` now updates a peer dependency that pnpm installed automatically [#10486](https://github.com/pnpm/pnpm/issues/10486).

- `pnpm update <pkg>` now moves a package off a locked version the registry no longer serves, such as an unpublished release. The lockfile check for supply-chain policies such as `minimumReleaseAge` used to reject that version before the update could replace it [#9953](https://github.com/pnpm/pnpm/issues/9953).

- `pnpm update --prod` no longer installs devDependencies when run in a project installed with `--prod` [#8038](https://github.com/pnpm/pnpm/issues/8038).

- `pnpm update --interactive --workspace` now allows external dependencies to be updated.

- `pnpm outdated` and `pnpm update` now apply `minimumReleaseAge` to GitHub Actions. `minimumReleaseAgeExclude` entries match action names such as `actions/checkout` [#13923](https://github.com/pnpm/pnpm/issues/13923).

- `pnpm remove` now runs the project's own `preuninstall`, `uninstall`, and `postuninstall` scripts. `preuninstall` and `uninstall` run before dependencies are unlinked. A failure in either stage aborts the removal. `postuninstall` runs after unlinking completes. The `ignoreScripts` setting and `--lockfile-only` skip all three stages [#3276](https://github.com/pnpm/pnpm/issues/3276).

- `pnpm remove -r` now fails if any requested dependency is absent from all selected workspace projects. Validation respects `--save-prod`, `--save-dev`, and `--save-optional` and completes before modifying project manifests [#2319](https://github.com/pnpm/pnpm/issues/2319).

- `pnpm remove` now accepts `--trust-lockfile` and `--no-trust-lockfile` to control supply-chain policy checks while removing a package [#14406](https://github.com/pnpm/pnpm/issues/14406).

- `pnpm unlink` now removes the `link:` dependency that `pnpm link <dir>` added to `package.json`. The linked package is removed from `node_modules` and the lockfile. A `link:` dependency to another directory is kept [#4219](https://github.com/pnpm/pnpm/issues/4219).

- `minimumReleaseAgeExcludePrune` and `trustPolicyExcludePrune` now work in workspaces with `shared-workspace-lockfile=false`. Once every project has been installed, pnpm drops an entry only if no project lockfile records it. Undecided `allowBuilds` entries are pruned the same way [#14612](https://github.com/pnpm/pnpm/issues/14612).

- `pnpm import` now converts dependencies that use Yarn's `patch:` protocol. The dependency keeps the version it patches, and the patch file is added to `patchedDependencies` in `pnpm-workspace.yaml`. If the patch file is missing, pnpm prints a warning and imports the dependency without the patch [#10278](https://github.com/pnpm/pnpm/issues/10278).

- `pnpm import` in a workspace now keeps the versions pinned by the root `yarn.lock`, `package-lock.json`, or `npm-shrinkwrap.json` when another workspace project's range allows a newer version. Before, the root project got the newest version in its range [#4385](https://github.com/pnpm/pnpm/issues/4385).

- `pnpm patch-commit` now resolves default patch directory locations when passed a package name or package specifier (such as `pnpm patch-commit <pkg>` or `pnpm patch-commit <pkg>@<version>`).

- `pnpm patch-commit` now updates the lockfile snapshot and prunes removed dependencies when the patch modifies `package.json` [#6866](https://github.com/pnpm/pnpm/issues/6866).

- `pnpm patch-commit` now falls back to copying package files when hard linking fails.

- `make-dedicated-lockfile` no longer removes fields such as `main` and `types` from the `publishConfig` of the project's `package.json`. It now restores `package.json` when it cannot move the original `node_modules` back to its place. The error then names `.tmp_node_modules`, where the original `node_modules` was left. The command refuses to run while that directory exists, so a retry cannot overwrite it.

#### Running scripts and commands

- A script that pnpm runs without a terminal now ends when pnpm itself is killed. Killing pnpm's process group, as Playwright's `webServer` does to stop the command it started, used to leave the script running and holding the caller's output pipes open [#15555](https://github.com/pnpm/pnpm/issues/15555).

- `pnpm --filter <project> <command>` and `pnpm -r <command>` now run a command installed in the selected projects' dependencies when none of them has a script by that name. This matches `pnpm <command>` in a single project. `pnpm run` with `--filter` or `-r` still reports the missing script [#10151](https://github.com/pnpm/pnpm/issues/10151).

- `pnpm exec` and `pnpm dlx` now set `npm_execpath`, `INIT_CWD`, `npm_node_execpath`, and `NODE` for child processes [#7037](https://github.com/pnpm/pnpm/issues/7037). Scripts that `pnpx` and `pnx` run now get pnpm itself as `npm_execpath`. A script that ran `$npm_execpath install` there ran `pnpm dlx install`.

- `pnpm exec` now sets the `PWD` environment variable to the directory the command runs in. Shells and tools that read `PWD` now report the logical path of a workspace package reached through a symlink [#1550](https://github.com/pnpm/pnpm/issues/1550).

- A script that runs `pnpm run` no longer adds duplicate `node_modules/.bin` and `node-gyp-bin` entries to `PATH` [#5352](https://github.com/pnpm/pnpm/issues/5352).

- Concurrent `pnpm run` and `pnpm exec` commands now serialize their dependency installs [#14551](https://github.com/pnpm/pnpm/issues/14551).

- `pnpm run --recursive` no longer reports interrupted scripts as lifecycle failures after `Ctrl+C`.

- `pnpm restart` now runs the "stop" and "start" scripts when the package has no "restart" script. Previously it ran "stop" and then failed with "Missing script: restart" [#4750](https://github.com/pnpm/pnpm/issues/4750).

- `pnpm install-test` now accepts `--no-bail` when executed directly and in recursive runs [#3777](https://github.com/pnpm/pnpm/issues/3777).

- `pnpm dlx` now keeps a separate cache entry for each Node.js major version. A package built under one Node.js major version, such as a native addon, is no longer reused under another [#8611](https://github.com/pnpm/pnpm/issues/8611).

- A `runtime:` version range that contains `||` or a space, such as a `devEngines.runtime` version of `^22.18.0 || ^24.0.0`, now installs the requested runtime. pnpm used to install the npm package with the same name, such as `node` [#14817](https://github.com/pnpm/pnpm/issues/14817).

- When the configured `scriptShell` does not exist, running a script now fails with an error that names the shell. Previously pnpm printed only an exit code or the package directory [#7562](https://github.com/pnpm/pnpm/issues/7562).

- If a script is killed by a signal that pnpm survives, such as SIGPIPE, the error now names the signal: `Command failed with signal SIGPIPE.` [#9821](https://github.com/pnpm/pnpm/issues/9821).

#### Publishing, packing, and deploying

- `pnpm publish` now resolves `workspace:` dependencies from workspace manifests when `node_modules` is not installed. Previously, publishing without `node_modules` failed with `ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL` [#6567](https://github.com/pnpm/pnpm/issues/6567).

- `pnpm publish` now honors `publishConfig["@scope:registry"]` for a package in that scope. It takes precedence over the registry set for the same scope in `.npmrc` and over `publishConfig.registry` [#12071](https://github.com/pnpm/pnpm/issues/12071).

- `pnpm pack` and `pnpm publish` now include bundled dependencies when using the isolated linker. This covers workspace packages and the dependencies of each bundled package. Bundled dependencies are also included when `publishConfig.directory` selects a build directory [#1643](https://github.com/pnpm/pnpm/issues/1643).

- `pnpm pack`, `pnpm deploy`, and installs of local directory dependencies now keep symlinks that point to files or directories included in the package. `pnpm pack` leaves out symlinks that point outside the package [#8208](https://github.com/pnpm/pnpm/issues/8208).

- `pnpm pack` now honors the `files` field of `package.yaml` and `package.json5` manifests. Git-hosted and injected local dependencies that use these manifests now honor it too [#7906](https://github.com/pnpm/pnpm/issues/7906). The archive includes exactly one `package.json` when the project uses an alternative manifest format, even when `.npmignore` or `files` excludes the source file.

- `pnpm pack` now preserves file executable permissions in the packed tarball when source files are executable on disk.

- `pnpm publish` and `pnpm pack` now report a missing `version` or `name` field on a workspace dependency. Previously, pnpm reported that the dependency was not installed [#4164](https://github.com/pnpm/pnpm/issues/4164).

- `pnpm publish` and `pnpm pack` now report an error when a bin script has a shebang line ending with CRLF [#7311](https://github.com/pnpm/pnpm/issues/7311).

- `pnpm deploy` now copies the `packageManager` and `devEngines.packageManager` fields of the workspace root `package.json` into the deployed `package.json`, unless the deployed project pins a package manager itself [#9079](https://github.com/pnpm/pnpm/issues/9079).

- `pnpm deploy` now puts the virtual store at `virtualStoreDir`, resolved against the deploy directory. A shared-lockfile deploy records `virtualStoreDir` in the deployed `pnpm-workspace.yaml`. With the global virtual store enabled or an absolute `virtualStoreDir`, the deploy still uses `node_modules/.pnpm` [#8787](https://github.com/pnpm/pnpm/issues/8787).

- `pnpm deploy` now respects `--package-import-method` passed on the command line and reports the package import method correctly [#7593](https://github.com/pnpm/pnpm/issues/7593).

- `pnpm deploy` no longer triggers an install when running scripts in a read-only deployed filesystem [#11617](https://github.com/pnpm/pnpm/issues/11617).

- A legacy `pnpm deploy` with `node-linker=hoisted` now puts the deployed project's direct dependencies at the top of the deployed `node_modules` [#9671](https://github.com/pnpm/pnpm/issues/9671).

- `pnpm deploy --legacy` no longer rewrites `node_modules/.pnpm-workspace-state-v1.json` in the source workspace. The next `verifyDepsBeforeRun` check there reported the workspace as out of date [#15352](https://github.com/pnpm/pnpm/issues/15352).

#### Manifests and configuration files

- Fixed `pnpm version` failing on projects using a `package.yaml` manifest.

  Fixed `pnpm init` creating an extra `package.json` when `package.yaml` is already present.

- pnpm now preserves CRLF line endings when it modifies project manifests.

- `pnpm version` now applies pending bumps to private workspace packages. A private package's changelog is written to its committed `CHANGELOG.md`, also when `versioning.changelog.storage` is `registry` [#13736](https://github.com/pnpm/pnpm/issues/13736), [#13519](https://github.com/pnpm/pnpm/issues/13519).

- `pnpm change check` now validates the pending change intents in `.changeset/`. It fails when an intent names a package that is not in the workspace or cannot be released.

- `.npmrc` files now support npm's `${VAR?}` placeholder. It expands to the value of `VAR`, or to an empty string without a warning when `VAR` is unset [#14404](https://github.com/pnpm/pnpm/issues/14404).

- pnpm now expands environment variables in `_auth.authToken` values loaded from global `config.yaml` and `pnpm_config__auth` [#12828](https://github.com/pnpm/pnpm/issues/12828).

- pnpm now warns when an empty environment variable removes an `.npmrc` authentication token. Authentication environment warnings now name the affected key [#4806](https://github.com/pnpm/pnpm/issues/4806).

- pnpm now keeps the configured default registry when `_auth` holds credentials for several registries and some of those registries serve package scopes.

  Lockfile verification checks a tarball hosted on a scoped registry against that registry's metadata, unless the package's own scope has a registry assigned [#15530](https://github.com/pnpm/pnpm/issues/15530).

- pnpm now treats a missing global `config.yaml`, `auth.ini`, or other optional config file as absent in Node.js-compatible runtimes such as StackBlitz WebContainers. Commands such as `pnpm --version` failed there with `ENOENT` [#14030](https://github.com/pnpm/pnpm/issues/14030).

- `pnpm login` now logs back in to an existing user on registries without web login, such as verdaccio. The classic login request sends the username and password as basic auth, as `npm login` does [#12055](https://github.com/pnpm/pnpm/issues/12055).

- `pnpm doctor` now checks the configured default registry and sends its credentials. It used to always ping `https://registry.npmjs.org/` [#15618](https://github.com/pnpm/pnpm/issues/15618).

#### Global packages, pnpm versions, and runtimes

- Global commands such as `pnpm add --global`, `pnpm list --global`, and `pnpm bin --global` now run with the pnpm you invoked, even in a project that pins another pnpm version. Previously, a pin with `onFail: "download"` switched them to the pinned pnpm, and a pinned pnpm 10 or older failed because its global bin directory was not in `PATH` [#14531](https://github.com/pnpm/pnpm/issues/14531).

- `pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` now recover a global package group whose entire `node_modules` directory was deleted. `pnpm remove -g` leaves such a group's command shims in the global bin directory [#15093](https://github.com/pnpm/pnpm/issues/15093). These commands no longer fail with `ERR_PNPM_PACKAGE_MANIFEST_IO_ERROR` when another global package's link into the store dangles, for example after the store was pruned. The pnpm install script failed the same way on such a machine.

- `pnpm update --global` now skips a global package installed from a `file:` path that no longer exists, prints a warning, and updates the remaining global packages. Previously the whole update failed with `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND` [#12533](https://github.com/pnpm/pnpm/issues/12533).

- `pnpm update -g` no longer asks more than once for approval of the same immature `name@version` when `minimumReleaseAgeStrict` is enabled [#15091](https://github.com/pnpm/pnpm/issues/15091).

- `pnpm self-update` run in a project that pins pnpm through `packageManager` or `devEngines.packageManager` now also updates the global pnpm, as it does outside a project [#14747](https://github.com/pnpm/pnpm/issues/14747).

- `pnpm self-update` no longer leaves the previous pnpm in the global packages when it was installed as `@pnpm/exe`. `pnpm ls -g` now lists a single pnpm [#14709](https://github.com/pnpm/pnpm/issues/14709).

- `pnpm setup` no longer deletes aliases and other lines that sit between a `# pnpm` comment and the pnpm block in a shell startup file [#7067](https://github.com/pnpm/pnpm/issues/7067).

- `pnpm env remove` now cleans up dangling Node.js executables and symlinks. Surviving global commands remain intact.

- Node.js runtime resolution now supports Windows ARM64. Node.js 20 and newer resolve native `win-arm64` builds, and older versions fall back to `win-x64` under emulation [#7123](https://github.com/pnpm/pnpm/issues/7123).

#### Windows and WSL

- `pn`, `pnpx`, `pnx`, and `pnpm` now run when Git Bash, MSYS2, or Cygwin launches them through a Windows path such as `C:\Users\me\node_modules\pnpm\pn`. The aliases used to fail to find the pnpm installed beside them, or hand the call to an unrelated one [#14884](https://github.com/pnpm/pnpm/issues/14884).

- Fallback `.cmd` and `.ps1` Windows wrappers in `@pnpm/exe` now propagate the exit status of the invoked `pnpm` command [#14826](https://github.com/pnpm/pnpm/issues/14826).

- On Windows, bin shims run from Git Bash, MSYS2, or Cygwin now pass `NODE_PATH` to Node.js as Windows paths. A project installed from cmd or PowerShell gave its bins a `NODE_PATH` under the Git install directory when they ran from Git Bash. Installing again replaces the shims already in `node_modules` [#3360](https://github.com/pnpm/pnpm/issues/3360).

- `pnpm install` in WSL now waits out Windows file locks on a Windows drive such as `/mnt/c`, as it already does on Windows. Before, an antivirus or indexer scan holding a file open could fail the install with `EACCES` [#6155](https://github.com/pnpm/pnpm/issues/6155).

- On Windows, pnpm now retries saving `pnpm-lock.yaml` for up to a minute while another process holds the file open. The save used to fail at once with `EPERM`, `EBUSY`, or "Access is denied" [#9461](https://github.com/pnpm/pnpm/issues/9461).

- On Windows, pnpm now fails within about a second when it cannot move a `node_modules` directory installed by another package manager because a file in it is in use. The error names the directory and suggests stopping the process that uses it. pnpm used to retry for a minute and then print a raw `EPERM` stack trace [#7505](https://github.com/pnpm/pnpm/issues/7505).

- pnpm now escapes trailing dots and spaces in `node_modules/.pnpm` directory names. Windows strips these characters, so a dependency such as `"parent-pkg": "file:../"` created a directory that could not be deleted or failed to install [#8101](https://github.com/pnpm/pnpm/issues/8101).

- `pnpm install` now resolves local tarballs specified with bare UNC paths on Windows [#1669](https://github.com/pnpm/pnpm/issues/1669).

- pnpm now recognizes local paths with forward slashes on Windows.

- On Windows, `pnpm add` and `pnpm update` now write relative `file:` and `link:` specifiers with forward slashes to `package.json` and the lockfile. They used to write backslashes, so the same project produced different files on Windows and on other systems [#7497](https://github.com/pnpm/pnpm/issues/7497), [#9687](https://github.com/pnpm/pnpm/issues/9687).

- Fixed scripts failing with errors such as `'an-compile' is not recognized` when `scriptShell` is set to `cmd.exe` on Windows [#7181](https://github.com/pnpm/pnpm/issues/7181).

#### Inspecting dependencies

- `pnpm audit` and `pnpm audit signatures` now check only the dependencies of the projects selected by `--filter`, `--filter-prod`, or `--workspace-root`. The filter used to be ignored, so a filtered audit reported the whole workspace [#10982](https://github.com/pnpm/pnpm/issues/10982).

- `pnpm audit` now lists at least one dependency path from every workspace project that depends on a vulnerable package. Before, a project whose dependency was reached through more than 100 paths filled the path list, and other projects that depend on the same package were left out [#12200](https://github.com/pnpm/pnpm/issues/12200).

- `pnpm audit --fix=update` now fixes vulnerabilities in dependencies declared through an npm alias. A specifier such as `"foo": "npm:vulnerable-pkg@1.0.0"` moves to the patched version and keeps the alias. Versions pinned with a leading `=` are fixed as well [#15155](https://github.com/pnpm/pnpm/issues/15155).

- `pnpm audit --fix` now prunes redundant overrides when one vulnerable range is a subset of another for the same package [#8577](https://github.com/pnpm/pnpm/issues/8577).

- Running `pnpm list` inside a workspace package without `--recursive` or a filter now lists only the current package [#14494](https://github.com/pnpm/pnpm/issues/14494). `pnpm licenses list` does the same. Use `--recursive` or `--filter` to list the licenses of other workspace projects [#5689](https://github.com/pnpm/pnpm/issues/5689).

- `pnpm list --only-projects` now prints every project selected with `--filter` or `--recursive`, including a project that has no workspace dependencies [#9770](https://github.com/pnpm/pnpm/issues/9770). It also lists the workspace projects when `sharedWorkspaceLockfile` is `false` [#7151](https://github.com/pnpm/pnpm/issues/7151), and a project that sets `publishConfig.directory` [#10635](https://github.com/pnpm/pnpm/issues/10635). It no longer reports packages in `node_modules` that are missing from the lockfile [#9528](https://github.com/pnpm/pnpm/issues/9528).

- `pnpm licenses list` failed or reported nothing in a workspace with `sharedWorkspaceLockfile: false`. It now reads the lockfile of each selected project [#10140](https://github.com/pnpm/pnpm/issues/10140).

- With `nodeLinker: hoisted`, `pnpm licenses list` reported paths under `node_modules/.pnpm` that do not exist. It now reports the directory where the hoisted linker placed each package [#8589](https://github.com/pnpm/pnpm/issues/8589).

- `pnpm outdated` and `pnpm -r outdated` now fail with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` when a requested package selector does not match any dependency in the inspected projects [#2319](https://github.com/pnpm/pnpm/issues/2319).

- `pnpm -r outdated --json` now includes every outdated workspace dependency when multiple projects depend on different versions or dependency types of the same package. Such a package is keyed by its current version and dependency type, for example `vue@2.7.14 (dev)` [#7693](https://github.com/pnpm/pnpm/issues/7693).

- `pnpm sbom` filtered to a single workspace project no longer replaces the project's own `license` or `bugs` field with the workspace root's value when the project's value is blank. The same applies to an `author`, `description`, `license`, `repository`, or `bugs` field set to `null` [#14882](https://github.com/pnpm/pnpm/issues/14882).

- `pnpm store status` no longer reports packages with build or postinstall scripts as modified in the store. When packages were mutated, it now lists only those packages and no longer suggests running `pnpm install --force` [#919](https://github.com/pnpm/pnpm/issues/919).

- `pnpm peers check` and the `ERR_PNPM_PEER_DEP_ISSUES` error now group peer dependency issues under the workspace project they were found in [#15351](https://github.com/pnpm/pnpm/issues/15351).

- The `pnpm:peer-dependency-issues` log event, which `--reporter ndjson` prints, no longer lists peers silenced by `peerDependencyRules.ignoreMissing` under `conflicts` or `intersections` [#8295](https://github.com/pnpm/pnpm/issues/8295).

#### Output and messages

- The error for an incompatible pnpm-lock.yaml now reports the lockfileVersion the file was generated with and the lockfileVersion the current pnpm supports. The error also warns that recreating the lockfile with `--force` may break the application and suggests installing the pnpm version that generated the lockfile [#848](https://github.com/pnpm/pnpm/issues/848).

- When the registry stops sending data for longer than `fetchTimeout`, pnpm now reports that the metadata or tarball request timed out. Previously the error did not mention the timeout [#3646](https://github.com/pnpm/pnpm/issues/3646).

- Resolution no longer logs an error when a package metadata request fails and resolution succeeds via cached metadata [#2522](https://github.com/pnpm/pnpm/issues/2522).

- The ignored build scripts warning and the update notice are printed as plain lines when output is not a terminal, in CI, or with `--reporter append-only`. They were drawn inside a box that broke apart in CI logs [#9421](https://github.com/pnpm/pnpm/issues/9421).

- `pnpm run` with `--loglevel` set to `warn`, `error`, or `silent` (or the same `loglevel` setting) no longer prints the `$ <command>` line before a script, nor the summary of the install that `verifyDepsBeforeRun` runs first. Both are info-level output [#8944](https://github.com/pnpm/pnpm/issues/8944).

- `pnpm add` now warns when replacing an existing dependency with a specifier pointing to a different source [#14869](https://github.com/pnpm/pnpm/issues/14869).

- `pnpm update` no longer warns "Skip adding ... to the default catalog" for a dependency that already uses `catalog:` [#13715](https://github.com/pnpm/pnpm/issues/13715).

- `pnpm remove --help` no longer shows a `[@<version>]` suffix in its usage line. The command accepts package names only [#7751](https://github.com/pnpm/pnpm/issues/7751).

- Bash completion now completes script names that contain a colon, such as `pnpm run test:u` to `pnpm run test:unit` [#5482](https://github.com/pnpm/pnpm/issues/5482).
