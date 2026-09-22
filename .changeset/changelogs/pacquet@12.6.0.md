## 12.6.0

### Minor Changes

- Added `pnpm add --save-types` to save available `@types/*` packages in `devDependencies` alongside registry dependencies. Packages that declare bundled TypeScript types are skipped. Set `saveTypes: true` in `pnpm-workspace.yaml` to enable this by default [pnpm/pnpm#3868](https://github.com/pnpm/pnpm/issues/3868).

- Added `autoDedupe` to deduplicate compatible dependency versions during installation. Enable it in `pnpm-workspace.yaml` or use `pnpm install --auto-dedupe` or `pnpm add --auto-dedupe`. Frozen installs leave the lockfile unchanged.

- Added `pnpm cache prune`, which deletes registry metadata cache directories that this version of pnpm can no longer read. Upgrading to pnpm 12.4.0 left one behind for every registry, and no command could remove them [#15046](https://github.com/pnpm/pnpm/issues/15046).

  `pnpm cache prune --dry-run` lists what it would delete without removing anything.

- Catalog entries can now use the `file:` and `link:` protocols. A relative path in an entry is measured from the directory holding `pnpm-workspace.yaml`, not from the project that references it. A bare path such as `./tarballs/foo.tgz` is measured from there too. It used to resolve against the project that referenced it [#8642](https://github.com/pnpm/pnpm/issues/8642).

- Waiting tasks of a concurrency group now take available slots in arrival order, with higher `priority` tasks going first. If workspaces use different limits for the same group, a later task can take a free slot that earlier tasks cannot use. `pnpm tasks status` lists running and waiting tasks in each group. It also shows how long each task has been running or waiting.

  A package script named `tasks` takes precedence. Use `pnpm pm tasks status` to inspect groups when that script exists.

- `pnpm add --tilde` is now an alias for `--save-prefix=~`. The Yarn `-T` shorthand is not supported.

- On macOS, `pnpm install` can now exclude newly created modules, virtual-store, and package-store
  directories from Time Machine. Set `macosBackup.excludeModulesDir` or `macosBackup.excludeStoreDir` to `true`
  in global YAML configuration. Environment variables `PNPM_CONFIG_MACOS_BACKUP_EXCLUDE_MODULES_DIR` and
  `PNPM_CONFIG_MACOS_BACKUP_EXCLUDE_STORE_DIR` are also supported. Project configuration cannot change
  these machine-local settings [pnpm/pnpm#6440](https://github.com/pnpm/pnpm/issues/6440).

- Added a `progress` setting and a `--no-progress` option that turn off the dependency and download progress lines. Warnings, lifecycle output, and the dependency summary are still printed.

- On macOS and Linux, project commands in `node_modules/.bin` now keep working after the project directory is moved or copied [#6937](https://github.com/pnpm/pnpm/issues/6937).

- `pnpm install`, `pnpm run`, and `pnpm exec` now reuse a `node_modules` directory that moved or was copied together with its project. The first command after the move checks the tree and records its new location. Reuse works on macOS and Linux, for a project moved with `mv`, `cp -a`, or a copy-on-write clone [#6937](https://github.com/pnpm/pnpm/issues/6937).

- `pnpm add`, `pnpm update`, `pnpm remove`, `pnpm pkg`, `pnpm link`, `pnpm set-script`, and `pnpm version` now update existing `package.yaml` manifests. Comments and existing key order are preserved.

### Patch Changes

- `pnpm install` now runs `pnpm:devPreinstall` when the root project uses `package.yaml` [pnpm/pnpm#15168](https://github.com/pnpm/pnpm/issues/15168).

- `pnpm add <pkg>` without a version now uses the catalog entry when the workspace already catalogs that package. Naming no version asks for whatever the workspace agreed on, so the entry stands even where it differs from the range `latest` would produce. `catalogMode: strict` used to fail with `ERR_PNPM_CATALOG_VERSION_MISMATCH` and `catalogMode: prefer` wrote a direct range into the manifest [pnpm/pnpm#14865](https://github.com/pnpm/pnpm/issues/14865).

- `pnpm change check` now validates the pending change intents in `.changeset/`. It fails when an intent names a package that is not in the workspace or cannot be released.

- `pnpm ci` now empties `node_modules` before installing in a project that declares a `clean` script. It ran that script in place of the removal [#15276](https://github.com/pnpm/pnpm/issues/15276).

- `pnpm store prune` now removes expired or superseded `pnpm dlx` cache data [#15171](https://github.com/pnpm/pnpm/issues/15171).

- `pnpm store prune` now removes unreferenced files and packages from the content-addressable store [#3635](https://github.com/pnpm/pnpm/issues/3635).

- Honor the `loglevel` setting when configured in `pnpm-workspace.yaml`, global configuration, or the `PNPM_CONFIG_LOGLEVEL` environment variable [#3122](https://github.com/pnpm/pnpm/issues/3122).

- Warn when `shared-workspace-lockfile` is passed on the command line outside a workspace.

- pnpm loads a configured `.js` pnpmfile as CommonJS or as an ES module, following the nearest `package.json`. The install used to fail with `ERR_PNPM_PNPMFILE_NOT_FOUND` for every `.js` pnpmfile [pnpm/pnpm#15141](https://github.com/pnpm/pnpm/issues/15141). An ES module pnpmfile may also export its hooks with `export default`.

- `pnpm install` now installs dependencies when a custom resolver returns a local or git-hosted tarball without a manifest [pnpm/pnpm#15016](https://github.com/pnpm/pnpm/issues/15016).

- `pnpm cache list-registries` now prints the registry URL, matching `pnpm cache view`. It printed `https%3A+registry.npmjs.org` before and prints `https://registry.npmjs.org/` now [#15046](https://github.com/pnpm/pnpm/issues/15046).

- Sped up `pnpm dedupe` and `pnpm install` in projects with many convergence overrides. The check for stale convergence overrides now runs its registry lookups for every override at once, so on a slow registry its cost no longer grows with the number of overrides [#15175](https://github.com/pnpm/pnpm/issues/15175).

- `pnpm deploy` no longer triggers an install when running scripts in a read-only deployed filesystem [#11617](https://github.com/pnpm/pnpm/issues/11617).

- Install warnings no longer carry the text of a package's deprecation notice. The warning names the deprecated package and version, and the `pnpm:deprecation` event no longer carries the notice either. `pnpm view` still shows it on request.

  A deprecation warning now names the newest version of the package that is not deprecated, and says when reaching it means widening the range you declared:

  ```
  WARN  deprecated foo@1.0.0. 2.3.1 is not deprecated, outside the range you declared.
  ```

  pnpm works this out from the metadata it already fetched, so it costs no extra request. An install that reuses the lockfile without fetching metadata names no version.

  pnpm strips control characters from the package name and version in a deprecation warning, and from the notice `pnpm outdated --long` prints.

  The text sanitizer now also strips the Unicode line and paragraph separators U+2028 and U+2029.

- `pnpm install` now uses the running Node.js when `devEngines.runtime` declares a range without `onFail: download`. Optional dependencies supported by the active Node.js are no longer skipped [pnpm/pnpm#15230](https://github.com/pnpm/pnpm/issues/15230).

- `pnpm dlx` now reuses cached packages when Windows creates directory junctions for its cache links.

- `pnpm-workspace.yaml` now expands environment variable placeholders with fallback syntax in enum-valued settings such as `nodeLinker` [#14914](https://github.com/pnpm/pnpm/issues/14914).

- pnpm now expands a leading `~/` in `storeDir` values loaded from the global config or `PNPM_CONFIG_STORE_DIR` to the user's home directory [pnpm/pnpm#6560](https://github.com/pnpm/pnpm/issues/6560).

- `pnpm publish` now allows a detached Git HEAD in CI, including checkouts of release tags. The working tree must still be clean. Branch and remote-history checks still apply when HEAD is attached [pnpm/pnpm#5894](https://github.com/pnpm/pnpm/issues/5894).

- pnpm no longer rewrites `packageManagerDependencies` in `pnpm-lock.yaml` when that block pins `@pnpm/exe` beside `pnpm`. The rewrite ran on every command, so `pnpm list` left a clean working tree dirty, and `pnpm version` then refused to run [#14926](https://github.com/pnpm/pnpm/issues/14926).

- Fixed shell completion for `pnpm --filter` and `pnpm -F` to suggest workspace package names [pnpm/pnpm#15216](https://github.com/pnpm/pnpm/issues/15216).

- `pnpm install --force` now removes obsolete dependency links inside virtual-store packages when their dependencies change. Invalid dependency names are ignored during obsolete-link cleanup [#15039](https://github.com/pnpm/pnpm/issues/15039).

- `pnpm add --global` now installs a local tarball when `PNPM_HOME` contains `..` path segments [#15118](https://github.com/pnpm/pnpm/issues/15118).

- `pnpm install` now runs dependency build scripts from long global virtual store paths on Windows [pnpm/pnpm#15111](https://github.com/pnpm/pnpm/issues/15111). Lifecycle spawn errors now include the refused working directory.

- Fixed scoped package paths used by the global virtual store and lifecycle script `PATH` on Windows.

- Fixed repeated `pnpm install --no-runtime --frozen-lockfile` failing with a broken lockfile when using `nodeLinker: hoisted` [#15212](https://github.com/pnpm/pnpm/issues/15212).

- Fixed `pnpm outdated` and `pnpm update --interactive --latest` omitting named-registry dependencies such as `work:2.1.0`. Updating these dependencies with `--latest` now preserves their registry prefix [pnpm/pnpm#15226](https://github.com/pnpm/pnpm/issues/15226).

- Fixed `pnpm peers check` and strict peer dependency checks rejecting compatible versions from named registries [#15225](https://github.com/pnpm/pnpm/issues/15225).

- Fixed `pnpm update --no-save` bypassing version-scoped overrides when a dependency selector specifies a version. The generated lockfile remains compatible with `pnpm install --frozen-lockfile` [pnpm/pnpm#14923](https://github.com/pnpm/pnpm/issues/14923).

- Fixed shell completion of package scripts for `pnpm run` and `pnpm run-script` [pnpm/pnpm#15034](https://github.com/pnpm/pnpm/issues/15034).

  Bash completion now preserves literal script names containing glob characters and shell punctuation in pnpm v11 and v12.

- `pnpm setup` now describes the displayed configuration changes as "the following configuration changes."

- `pnpm version` now reads `tagVersionPrefix` from `pnpm-workspace.yaml` and the global config file. An empty value removes the `v` prefix. `PNPM_CONFIG_TAG_VERSION_PREFIX` sets the same value from the environment. The `--tag-version-prefix` flag still overrides the configured value [#15044](https://github.com/pnpm/pnpm/issues/15044).

- `pn`, `pnpx`, `pnx`, and `pnpm` now run when Git Bash, MSYS2, or Cygwin launches them through a Windows path such as `C:\Users\me\node_modules\pnpm\pn`. The aliases used to fail to find the pnpm installed beside them, or hand the call to an unrelated one [#14884](https://github.com/pnpm/pnpm/issues/14884).

- pnpm now links workspace dependencies declared with plain version ranges when `excludeLinksFromLockfile` and `linkWorkspacePackages` are enabled.

- `pnpm install --force` now re-imports every package into the virtual store, so it repairs a package whose files have drifted. A forced install kept the files an earlier install left in place [#15030](https://github.com/pnpm/pnpm/issues/15030).

- `pnpm test` now forwards `--filter` arguments to the test script when the option follows the shortcut. [#15217](https://github.com/pnpm/pnpm/issues/15217)

- `pnpm install --frozen-lockfile` now succeeds when an optional dependency was unresolvable and skipped by the install that wrote the lockfile. Previously, frozen installs failed with `ERR_PNPM_OUTDATED_LOCKFILE`. The notice states that the dependency could not be resolved and names the requested range [pnpm/pnpm#3960](https://github.com/pnpm/pnpm/issues/3960).

- Fixed `pnpm install --frozen-lockfile` installing dependencies of projects removed from `pnpm-workspace.yaml`. Missing local tarballs used only by those projects no longer fail the install [#15248](https://github.com/pnpm/pnpm/issues/15248).

- `pnpm install` now installs git-hosted dependencies without preparing them when their builds are explicitly denied by `allowBuilds`. Dependencies that require preparation still need an explicit allow or deny decision [pnpm/pnpm#10522](https://github.com/pnpm/pnpm/issues/10522).

- `pnpm install` no longer appears to hang when a git dependency is fetched over SSH and ssh asks for a key passphrase or a host key confirmation. pnpm now runs ssh in batch mode, so the install fails right away with the ssh error, and a key that needs a passphrase has to be loaded into an SSH agent first. An ssh command selected through `GIT_SSH_COMMAND`, `GIT_SSH`, or the `core.sshCommand` git setting is kept as is [#2227](https://github.com/pnpm/pnpm/issues/2227).

- `pnpm install` now applies changes to or removal of a global `readPackage` hook when an existing lockfile is present [pnpm/pnpm#15136](https://github.com/pnpm/pnpm/issues/15136).

- `pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` now recover a global package group whose entire `node_modules` directory was deleted. `pnpm remove -g` leaves such a group's command shims in the global bin directory [#15093](https://github.com/pnpm/pnpm/issues/15093).

- `pnpm update -g` no longer asks more than once for approval of the same immature `name@version` when `minimumReleaseAgeStrict` is enabled [#15091](https://github.com/pnpm/pnpm/issues/15091).

- Fixed `pnpm install` not re-linking a package's global virtual store slot after `allowBuilds` changed [#15117](https://github.com/pnpm/pnpm/issues/15117).

- Workspace projects that `hoistPattern` or `publicHoistPattern` selects are now hoisted on every install. A project added to the workspace was not hoisted until `node_modules` was deleted and reinstalled. A workspace that installs nothing from a registry hoisted none of its projects at all [#3642](https://github.com/pnpm/pnpm/issues/3642).

- The install summary now names the version each dependency resolved to when `node-linker` is `hoisted`. Dependencies restored after `node_modules` is deleted appear in the summary. Unsupported optional dependencies removed from `node_modules` also appear. Version changes show both the old and new versions [#15161](https://github.com/pnpm/pnpm/issues/15161).

- `pnpm install-test` now accepts `--no-bail` when executed directly and in recursive runs [#3777](https://github.com/pnpm/pnpm/issues/3777).

- `pnpm install` now reads the same local tarball it installs when a dependency's absolute `file:` path contains `..`. Such a path could install a different tarball than the one it read, failing with `ERR_PNPM_TARBALL_INTEGRITY`, or fail to resolve at all.

- Preserve CRLF line endings when modifying project manifests.

- Fixed command lookup for a custom `modulesDir` in `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. `pnpm bin` now reports the configured executable directory. In a workspace whose projects keep their own lockfiles, a `packageConfigs` entry that gives one project its own `modulesDir` is followed too. Only a directory name is supported, not a `modulesDir` holding a path separator [#3604](https://github.com/pnpm/pnpm/issues/3604).

- Tools installed in a custom `modulesDir` can load CommonJS plugins installed there, the same way they would from `node_modules`. When executables are symlinks, as with `preferSymlinkedExecutables` or the hoisted linker, this works for `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. A symlinked tool started directly from a shell does not get it. Paths containing the platform path-list separator do not receive this fallback. `extendNodePath: false` disables this fallback [#3604](https://github.com/pnpm/pnpm/issues/3604).

- pnpm now measures a `pnpm.overrides` entry written as a bare path, such as `./local-dep`, from the directory holding `pnpm-workspace.yaml`. It used to be measured from each package the override rewrote, so the dependency linked to a directory that does not exist [#11131](https://github.com/pnpm/pnpm/issues/11131).

- A `package.yaml` project now records its pinned pnpm under `packageManagerDependencies` in `pnpm-lock.yaml` [pnpm/pnpm#15167](https://github.com/pnpm/pnpm/issues/15167).

- `pnpm peers check`, `why`, `list`, `ll`, `licenses`, `audit`, `sbom`, `fetch`, `patch`, `patch-commit`, `patch-remove`, `approve-builds` and `runtime` now honor settings applied by an `updateConfig` hook. Hook-provided catalogs now resolve the `catalog:` peer dependencies of linked workspace packages in `pnpm peers check` [pnpm/pnpm#15047](https://github.com/pnpm/pnpm/issues/15047). `pnpm fetch` previously failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when a hook supplied the catalog recorded in the lockfile. It now honors hook-provided catalogs when checking the lockfile config [pnpm/pnpm#15049](https://github.com/pnpm/pnpm/pull/15049).

- `pnpm pipeline --watch` now names the agent's checkout after the repository when `--repo` is given a Windows path. The Cargo build cache resolves the project and repository paths before it keys an entry, so two spellings of one directory, such as a Windows 8.3 short path, share the cached build state [#15105](https://github.com/pnpm/pnpm/issues/15105).

- `pnpm deploy`, `pnpm rebuild`, `pnpm rb`, and `pnpm setup` now run a `package.json` script of the same name, as documented. `pnpm pm <name>` still forces the built-in command [pnpm/pnpm#14976](https://github.com/pnpm/pnpm/issues/14976).

- pnpm now preserves scalar YAML anchors and aliases when editing `pnpm-workspace.yaml`. Removing the entry that defines an anchor keeps surviving aliases valid. Entries updated to different values are written separately [#8245](https://github.com/pnpm/pnpm/issues/8245).

- `pnpm install --prod` no longer downloads the registry packages that only a devDependency reaches [#881](https://github.com/pnpm/pnpm/issues/881).

- `pnpm update --global` no longer reinstalls a global package when its dependency graph resolves to what is already installed. It reports `Already up to date` [pnpm/pnpm#12002](https://github.com/pnpm/pnpm/issues/12002).

- The `minimumReleaseAge` approval prompt now counts and displays each package version once [pnpm/pnpm#15083](https://github.com/pnpm/pnpm/issues/15083).

- `pnpm run` no longer sends a script a second `SIGINT` when `Ctrl+C` is pressed in a terminal. A script that shuts down on the first `SIGINT` and exits at once on a second used to die before its shutdown finished [#7374](https://github.com/pnpm/pnpm/issues/7374).

- Warn when an empty environment variable removes an `.npmrc` authentication token. Authentication environment warnings now name the affected key [pnpm/pnpm#4806](https://github.com/pnpm/pnpm/issues/4806).

- `pnpm --version` no longer creates a temporary file in the project directory during store detection.

- Removing a `readPackage` hook from a project's pnpmfile now takes the dependencies it added back out of `pnpm-lock.yaml` [#3735](https://github.com/pnpm/pnpm/issues/3735).

- `pnpm -r list --json` now prints one JSON array. It printed a separate array for each project when `sharedWorkspaceLockfile` was `false`, so the output could not be parsed.

  `pnpm -r list` now reads each project's own modules directory when the projects keep their own lockfiles, so `--long` and `--parseable` report the packages that project installed [#15011](https://github.com/pnpm/pnpm/issues/15011).

- Recursive runs now start the scripts a `/pattern/` selector matched in one package at the same time. `pnpm --parallel` starts all of them. Other recursive runs keep the number of scripts running at once within `workspaceConcurrency`. The matched scripts previously ran one after another [pnpm/pnpm#14933](https://github.com/pnpm/pnpm/issues/14933).

- `pnpm remove -r` now fails if any requested dependency is absent from all selected workspace projects. Validation respects `--save-prod`, `--save-dev`, and `--save-optional` and completes before modifying project manifests [#2319](https://github.com/pnpm/pnpm/issues/2319).

- A signal sent to pnpm while it runs without a terminal, as a container runtime or a service manager does, now reaches the script even when the shell running it stays the script's parent. pnpm then waits for the script to finish shutting down. Such a signal used to end the shell at once or stay with it, and the script was never told to stop [#7374](https://github.com/pnpm/pnpm/issues/7374).

- Fixed `minimumReleaseAge` being skipped for packages served by a registry that returns the same ETag for abbreviated and full package metadata [pnpm/pnpm#14925](https://github.com/pnpm/pnpm/issues/14925).

- `pnpm remove` now runs the project's own `preuninstall`, `uninstall`, and `postuninstall` scripts. `preuninstall` and `uninstall` run before dependencies are unlinked. A failure in either stage aborts the removal. `postuninstall` runs after unlinking completes. The `ignoreScripts` setting and `--lockfile-only` skip all three stages [#3276](https://github.com/pnpm/pnpm/issues/3276).

- A dependency whose `file:` specifier is an absolute path containing `..` no longer makes every repeat install redo the work of a first one.

- Installs in different projects that share a global virtual store no longer fail on Windows with `Access is denied` while repairing the same slot [#15114](https://github.com/pnpm/pnpm/issues/15114).

- Installs in different projects that share a global virtual store no longer fail on Windows with `Cannot create a file when that file already exists` while linking the same dependency [#15176](https://github.com/pnpm/pnpm/issues/15176).

- `pnpm install` now reuses a tarball download that is already in flight when another resolution of the same archive still needs its package.json [#15037](https://github.com/pnpm/pnpm/issues/15037).

- The root project's `preinstall` script now runs before dependencies are resolved and linked. A guard such as `npx only-allow yarn` can stop the install before pnpm populates `node_modules` [#3760](https://github.com/pnpm/pnpm/issues/3760).

- Shell completion now omits candidates containing control or invisible formatting characters. Package and script names can no longer inject extra completion records or terminal escape sequences.

- `pnpm sbom` now emits a license value that is not an SPDX identifier, such as `UNLICENSED` or a misspelled identifier, as a CycloneDX license name. Such a value used to be emitted as a `license.id`, which made the whole CycloneDX document fail schema validation [pnpm/pnpm#14786](https://github.com/pnpm/pnpm/issues/14786).

- `pnpm sbom` now emits a license value as a CycloneDX expression only when it is a valid SPDX license expression. Anything else is emitted as a CycloneDX license name [pnpm/pnpm#14786](https://github.com/pnpm/pnpm/issues/14786).

- `pnpm install` and other commands that report configuration warnings now warn when environment variables in project `.npmrc` credentials are ignored. The warning links to the npmrc documentation [pnpm/pnpm#15051](https://github.com/pnpm/pnpm/issues/15051).

- A command run in a project that the workspace does not include now acts on that project alone. A project is outside the workspace when it has a manifest of its own and no pattern in the `packages` setting selects it, or when a `!` pattern excludes it. A directory with no manifest of its own, such as a package's source directory, still belongs to the workspace. `pnpm install` in an excluded project used to install every project in the workspace [#3561](https://github.com/pnpm/pnpm/issues/3561).

- `pnpm install` now enforces the root project's `engines.node` range when `engineStrict` is enabled [#3016](https://github.com/pnpm/pnpm/issues/3016).

- POSIX bin shims now take `cygpath` and `wslpath` from the system default path on Cygwin, MSYS2, and WSL2. The shims looked both helpers up on `PATH`, where a dependency's own bins come first, so a dependency could redirect another package's shim. Installing again replaces the shims already in `node_modules` [#14866](https://github.com/pnpm/pnpm/issues/14866).

- Added a `--peer` flag to `pnpm update` to update ranges in `peerDependencies` [#8081](https://github.com/pnpm/pnpm/issues/8081).

- `pnpm update` now moves the `devEngines.runtime` and `engines.runtime` version ranges to the resolved Node.js version [#14988](https://github.com/pnpm/pnpm/issues/14988).

- `pnpm install` and `pnpm update` now resolve a dependency range to the newest matching version that is not deprecated. A version already recorded in the lockfile is still used [#15128](https://github.com/pnpm/pnpm/issues/15128).

- Filtered and recursive run and exec commands in workspaces with `sharedWorkspaceLockfile: false` now verify dependencies in the selected projects rather than expecting a root workspace state [pnpm/pnpm#15272](https://github.com/pnpm/pnpm/issues/15272).

- `pnpm version` now uses the configured `tagVersionPrefix` when creating and reading Git tags [#15044](https://github.com/pnpm/pnpm/issues/15044).

- Fixed concurrent installs on Windows failing with a file-sharing error while inspecting packages in a shared global virtual store.

- Fixed workspace packages with SemVer build metadata being skipped when they match the requested range and have the same version precedence as the registry package [#2812](https://github.com/pnpm/pnpm/issues/2812).
