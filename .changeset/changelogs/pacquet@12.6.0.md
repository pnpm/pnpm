## 12.6.0

pnpm 12.6.0 ships with automatic dependency deduplication, relocatable node_modules, package.yaml manifest editing, and --save-types support.

### Minor Changes

- `autoDedupe` deduplicates compatible dependency versions during installation [#7258](https://github.com/pnpm/pnpm/issues/7258). Enable it in `pnpm-workspace.yaml` or use `pnpm install --auto-dedupe` or `pnpm add --auto-dedupe`. Frozen installs leave the lockfile unchanged.

- `pnpm install`, `pnpm run`, and `pnpm exec` on macOS and Linux now reuse a `node_modules` directory and bin shims that moved or were copied together with their project [#6937](https://github.com/pnpm/pnpm/issues/6937). The first command after the move checks the tree and records its new location, so project commands in `node_modules/.bin` keep working.

- `pnpm add --save-types` saves available `@types/*` packages in `devDependencies` alongside registry dependencies [#3868](https://github.com/pnpm/pnpm/issues/3868). Packages that declare bundled TypeScript types are skipped. Set `saveTypes: true` in `pnpm-workspace.yaml` to enable this by default.

- `package.yaml` manifests can now be updated by `pnpm add`, `pnpm update`, `pnpm remove`, `pnpm pkg`, `pnpm link`, `pnpm set-script`, and `pnpm version` [#2008](https://github.com/pnpm/pnpm/issues/2008). Existing comments and key order are preserved.

- Catalog entries can now use the `file:` and `link:` protocols [#8642](https://github.com/pnpm/pnpm/issues/8642). A relative path or bare path in an entry, such as `./tarballs/foo.tgz`, is measured from the directory holding `pnpm-workspace.yaml`.

- `pnpm tasks status` lists running and waiting tasks in each concurrency group, and waiting tasks now take available slots in arrival order with higher `priority` tasks going first [#15208](https://github.com/pnpm/pnpm/issues/15208). If workspaces use different limits for the same group, a later task can take a free slot that earlier tasks cannot use. A package script named `tasks` takes precedence; use `pnpm pm tasks status` when that script exists.

- `pnpm cache prune` deletes registry metadata cache directories that this version of pnpm can no longer read [#15046](https://github.com/pnpm/pnpm/issues/15046). `pnpm cache prune --dry-run` lists what it would delete without removing anything.

- `macosBackup.excludeModulesDir` and `macosBackup.excludeStoreDir` on macOS can now exclude newly created modules, virtual-store, and package-store directories from Time Machine [#6440](https://github.com/pnpm/pnpm/issues/6440). Set either to `true` in global configuration or using the `PNPM_CONFIG_MACOS_BACKUP_EXCLUDE_MODULES_DIR` and `PNPM_CONFIG_MACOS_BACKUP_EXCLUDE_STORE_DIR` environment variables.

- `pnpm add --tilde` is now an alias for `--save-prefix=~` [#12863](https://github.com/pnpm/pnpm/issues/12863). The Yarn `-T` shorthand is not supported.

- `progress` setting and `--no-progress` option now turn off dependency and download progress lines [#14065](https://github.com/pnpm/pnpm/issues/14065). Warnings, lifecycle output, and the dependency summary are still printed.

### Patch Changes

#### Security

- POSIX bin shims now take `cygpath` and `wslpath` from the system default path on Cygwin, MSYS2, and WSL2 so a dependency cannot redirect another package's shim [#14866](https://github.com/pnpm/pnpm/issues/14866).

- `pnpm install` warnings no longer carry the text of a package's deprecation notice, naming only the deprecated package and version [#15099](https://github.com/pnpm/pnpm/issues/15099). A deprecation warning names the newest non-deprecated version when one exists, and control characters and line separators are stripped from package identifiers and warnings.

- `pnpm install` and other commands that report configuration warnings now warn when environment variables in project `.npmrc` credentials are ignored [#15051](https://github.com/pnpm/pnpm/issues/15051).

#### Installing packages

- `pnpm install --frozen-lockfile` now succeeds when an optional dependency was unresolvable and skipped by the install that wrote the lockfile [#3960](https://github.com/pnpm/pnpm/issues/3960).

- `pnpm install --frozen-lockfile` no longer installs dependencies of projects removed from `pnpm-workspace.yaml` [#15248](https://github.com/pnpm/pnpm/issues/15248). Missing local tarballs used only by those projects no longer fail the install.

- `pnpm ci` now empties `node_modules` before installing in a project that declares a `clean` script [#15276](https://github.com/pnpm/pnpm/issues/15276).

- `pnpm install --force` now re-imports every package into the virtual store [#15030](https://github.com/pnpm/pnpm/issues/15030) and removes obsolete dependency links inside virtual-store packages when their dependencies change [#15039](https://github.com/pnpm/pnpm/issues/15039).

- `preinstall` script for the root project now runs before dependencies are resolved and linked [#3760](https://github.com/pnpm/pnpm/issues/3760).

- `pnpm install` now runs `pnpm:devPreinstall` when the root project uses `package.yaml` [#15168](https://github.com/pnpm/pnpm/issues/15168).

- `pnpm install` now enforces the root project's `engines.node` range when `engineStrict` is enabled [#3016](https://github.com/pnpm/pnpm/issues/3016).

- `pnpm install` now uses the running Node.js when `devEngines.runtime` declares a range without `onFail: download` [#15230](https://github.com/pnpm/pnpm/issues/15230).

- `pnpm install` no longer hangs when a git dependency is fetched over SSH and ssh prompts for a passphrase or host key confirmation, running ssh in batch mode instead [#2227](https://github.com/pnpm/pnpm/issues/2227).

- `pnpm install` now installs git-hosted dependencies without preparing them when their builds are explicitly denied by `allowBuilds` [#10522](https://github.com/pnpm/pnpm/issues/10522).

- `pnpm install` now reuses an in-flight tarball download when another resolution of the same archive still needs its `package.json` [#15037](https://github.com/pnpm/pnpm/issues/15037).

- `pnpm install --prod` no longer downloads registry packages that only a devDependency reaches [#881](https://github.com/pnpm/pnpm/issues/881).

- `pnpm install --no-runtime --frozen-lockfile` with `nodeLinker: hoisted` no longer fails on repeated runs with a broken lockfile [#15212](https://github.com/pnpm/pnpm/issues/15212).

#### Resolving and linking dependencies

- `pnpm install` and `pnpm update` now resolve a dependency range to the newest matching version that is not deprecated [#15128](https://github.com/pnpm/pnpm/issues/15128).

- `pnpm add <pkg>` without a version now uses the catalog entry when the workspace already catalogs that package [#14865](https://github.com/pnpm/pnpm/issues/14865).

- `pnpm install` now links workspace dependencies declared with plain version ranges when `excludeLinksFromLockfile` and `linkWorkspacePackages` are enabled [#15133](https://github.com/pnpm/pnpm/issues/15133).

- `pnpm install` now resolves local tarball dependencies whose absolute `file:` paths contain `..` consistently and skips reinstallation on repeat installs [#15190](https://github.com/pnpm/pnpm/issues/15190).

- `pnpm install` now installs dependencies when a custom resolver returns a local or git-hosted tarball without a manifest [#15016](https://github.com/pnpm/pnpm/issues/15016).

- `pnpm.overrides` entries written as a bare path, such as `./local-dep`, are now measured from the directory holding `pnpm-workspace.yaml` [#11131](https://github.com/pnpm/pnpm/issues/11131).

- `pnpm update --no-save` no longer bypasses version-scoped overrides when a dependency selector specifies a version [#14923](https://github.com/pnpm/pnpm/issues/14923).

- `pnpm peers check` and strict peer dependency checks no longer reject compatible versions from named registries [#15225](https://github.com/pnpm/pnpm/issues/15225).

- `pnpm outdated` and `pnpm update --interactive --latest` now include named-registry dependencies such as `work:2.1.0` and preserve their registry prefix [#15226](https://github.com/pnpm/pnpm/issues/15226).

- Workspace projects selected by `hoistPattern` or `publicHoistPattern` are now hoisted on every install [#3642](https://github.com/pnpm/pnpm/issues/3642).

- Workspace packages with SemVer build metadata are no longer skipped when they match the requested range and have the same version precedence as the registry package [#2812](https://github.com/pnpm/pnpm/issues/2812).

- Sped up `pnpm dedupe` and `pnpm install` in projects with many convergence overrides by checking overrides concurrently [#15175](https://github.com/pnpm/pnpm/issues/15175).

- `minimumReleaseAge` is no longer skipped for packages served by registries returning matching ETags for abbreviated and full package metadata [#14925](https://github.com/pnpm/pnpm/issues/14925).

#### Running scripts and tasks

- `pnpm run` signal handling no longer delivers a redundant second `SIGINT` to child scripts on `Ctrl+C` in a terminal, and properly forwards termination signals when running non-interactively without a terminal [#7374](https://github.com/pnpm/pnpm/issues/7374).

- `pnpm run` and `pnpm exec` in workspaces with `sharedWorkspaceLockfile: false` now verify dependencies in the selected projects rather than expecting a root workspace state [#15272](https://github.com/pnpm/pnpm/issues/15272).

- `pnpm test` now forwards `--filter` arguments to the test script when the option follows the shortcut [#15217](https://github.com/pnpm/pnpm/issues/15217).

- Recursive runs now start scripts matched by a `/pattern/` selector in parallel within `workspaceConcurrency` [#14933](https://github.com/pnpm/pnpm/issues/14933).

- `pnpm deploy`, `pnpm rebuild`, `pnpm rb`, and `pnpm setup` now prefer a `package.json` script of the same name [#14976](https://github.com/pnpm/pnpm/issues/14976).

- `modulesDir` custom directory names now support executable lookup and CommonJS plugin resolution across `pnpm run`, `pnpm exec`, `pnpm version` hooks, and lifecycle scripts [#3604](https://github.com/pnpm/pnpm/issues/3604).

- `pnpm install-test` now accepts `--no-bail` directly and in recursive runs [#3777](https://github.com/pnpm/pnpm/issues/3777).

#### Workspace and project configuration

- `pnpm` commands run in a project not included in the workspace now act on that project alone [#3561](https://github.com/pnpm/pnpm/issues/3561).

- `pnpm-workspace.yaml` edits now preserve scalar YAML anchors and aliases [#8245](https://github.com/pnpm/pnpm/issues/8245).

- `pnpm-workspace.yaml` now expands environment variable placeholders with fallback syntax in enum-valued settings such as `nodeLinker` [#14914](https://github.com/pnpm/pnpm/issues/14914).

- `pnpmfile` configuration now loads a `.js` file as CommonJS or an ES module, following the nearest `package.json` [#15141](https://github.com/pnpm/pnpm/issues/15141).

- `updateConfig` hook settings are now honored by `pnpm peers check`, `why`, `list`, `ll`, `licenses`, `audit`, `sbom`, `fetch`, `patch`, `patch-commit`, `patch-remove`, `approve-builds`, and `runtime` [#15047](https://github.com/pnpm/pnpm/issues/15047), [#15049](https://github.com/pnpm/pnpm/issues/15049).

- `readPackage` hook changes or removal now take added dependencies out of `pnpm-lock.yaml` and update dependencies when an existing lockfile is present [#3735](https://github.com/pnpm/pnpm/issues/3735), [#15136](https://github.com/pnpm/pnpm/issues/15136).

- `package.yaml` projects now record their pinned pnpm under `packageManagerDependencies` in `pnpm-lock.yaml` [#15167](https://github.com/pnpm/pnpm/issues/15167).

- `packageManagerDependencies` pinning `@pnpm/exe` beside `pnpm` is no longer rewritten in `pnpm-lock.yaml` [#14926](https://github.com/pnpm/pnpm/issues/14926).

- `pnpm` now preserves CRLF line endings when modifying project manifests [#3529](https://github.com/pnpm/pnpm/issues/3529).

- `loglevel` setting is now honored when configured in `pnpm-workspace.yaml`, global configuration, or `PNPM_CONFIG_LOGLEVEL` [#3122](https://github.com/pnpm/pnpm/issues/3122).

- `storeDir` values loaded from global configuration or `PNPM_CONFIG_STORE_DIR` now expand a leading `~/` to the user's home directory [#6560](https://github.com/pnpm/pnpm/issues/6560).

- `--shared-workspace-lockfile` now produces a warning when passed on the command line outside a workspace [#1617](https://github.com/pnpm/pnpm/issues/1617).

#### Windows

- `pnpm install` on Windows now runs dependency build scripts from long global virtual store paths and normalizes scoped package paths in lifecycle script `PATH` entries [#15111](https://github.com/pnpm/pnpm/issues/15111).

- `pnpm install` across projects sharing a global virtual store on Windows no longer fails with `Access is denied`, file-exists errors, or transient sharing violations [#15114](https://github.com/pnpm/pnpm/issues/15114), [#15176](https://github.com/pnpm/pnpm/issues/15176), [#15171](https://github.com/pnpm/pnpm/issues/15171).

- `pn`, `pnpx`, `pnx`, and `pnpm` now run when Git Bash, MSYS2, or Cygwin launches them through a Windows path [#14884](https://github.com/pnpm/pnpm/issues/14884).

- `pnpm dlx` now reuses cached packages when Windows creates directory junctions for its cache links [#15171](https://github.com/pnpm/pnpm/issues/15171).

- `pnpm pipeline --watch` now resolves Windows short paths so multiple path representations share the build cache [#15105](https://github.com/pnpm/pnpm/issues/15105).

#### CLI commands and output

- `pnpm remove` now runs the project's own `preuninstall`, `uninstall`, and `postuninstall` scripts [#3276](https://github.com/pnpm/pnpm/issues/3276).

- `pnpm remove -r` now fails before modifying manifests if any requested dependency is absent from all selected projects [#2319](https://github.com/pnpm/pnpm/issues/2319).

- `pnpm update --peer` now updates ranges in `peerDependencies` [#8081](https://github.com/pnpm/pnpm/issues/8081).

- `pnpm update` now moves `devEngines.runtime` and `engines.runtime` version ranges to the resolved Node.js version [#14988](https://github.com/pnpm/pnpm/issues/14988).

- `pnpm update -g` no longer reinstalls unchanged packages [#12002](https://github.com/pnpm/pnpm/issues/12002).

- `pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` now recover a global package group whose `node_modules` directory was deleted [#15093](https://github.com/pnpm/pnpm/issues/15093).

- `pnpm add -g` now installs local tarballs when `PNPM_HOME` contains `..` path segments [#15118](https://github.com/pnpm/pnpm/issues/15118).

- `pnpm version` now reads `tagVersionPrefix` from `pnpm-workspace.yaml`, global config, or `PNPM_CONFIG_TAG_VERSION_PREFIX` when creating and reading Git tags [#15044](https://github.com/pnpm/pnpm/issues/15044).

- `pnpm publish` now allows a detached Git HEAD in CI environments [#5894](https://github.com/pnpm/pnpm/issues/5894).

- `pnpm store prune` now removes unreferenced files and packages from the content-addressable store [#3635](https://github.com/pnpm/pnpm/issues/3635), as well as expired or superseded `pnpm dlx` cache data [#15171](https://github.com/pnpm/pnpm/issues/15171).

- `pnpm cache list-registries` now prints decoded registry URLs [#15046](https://github.com/pnpm/pnpm/issues/15046).

- `pnpm deploy` no longer triggers an install when running scripts in a read-only deployed filesystem [#11617](https://github.com/pnpm/pnpm/issues/11617).

- `pnpm -r list --json` now outputs a single JSON array when `sharedWorkspaceLockfile` is `false`, and `--long` and `--parseable` read each project's own modules directory [#15011](https://github.com/pnpm/pnpm/issues/15011).

- `pnpm sbom` now validates SPDX identifiers and expressions before emitting them as CycloneDX license IDs or expressions, falling back to a license name for non-SPDX values such as `UNLICENSED` [#14786](https://github.com/pnpm/pnpm/issues/14786).

- `pnpm change check` now validates pending change intents in `.changeset/` [#15183](https://github.com/pnpm/pnpm/issues/15183).

- `pnpm --filter` and `pnpm -F` shell completion now suggests workspace package names [#15216](https://github.com/pnpm/pnpm/issues/15216). Completion candidates containing control or invisible formatting characters are omitted so package and script names cannot inject terminal escape sequences.

- `pnpm run` and `pnpm run-script` shell completion now suggests package scripts [#15034](https://github.com/pnpm/pnpm/issues/15034).

- `pnpm --version` no longer creates a temporary file in the project directory during store detection [#15264](https://github.com/pnpm/pnpm/issues/15264).

- `pnpm setup` now describes displayed configuration changes as "The following configuration changes were made" [#15100](https://github.com/pnpm/pnpm/issues/15100).

- `minimumReleaseAge` approval prompts in `pnpm install` and `pnpm update -g` now count and display each package version once [#15083](https://github.com/pnpm/pnpm/issues/15083), [#15091](https://github.com/pnpm/issues/15091).

- `.npmrc` authentication warnings now report when an empty environment variable removes an auth token and name the affected key [#4806](https://github.com/pnpm/pnpm/issues/4806).

- The install summary now names the version each dependency resolved to when `node-linker` is `hoisted` [#15161](https://github.com/pnpm/pnpm/issues/15161).

- `pnpm install` now re-links a package's global virtual store slot after `allowBuilds` changes [#15117](https://github.com/pnpm/pnpm/issues/15117).
