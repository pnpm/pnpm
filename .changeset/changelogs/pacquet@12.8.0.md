## 12.8.0

pnpm 12.8.0 warns when `pnpm pack` or `pnpm publish` would ship a `.env` file that `files` does not list, installs `sharedWorkspaceLockfile: false` workspaces concurrently, applies every setting passed as `--config.<name>=<value>`, and no longer leaves the Windows terminal stuck after Ctrl+C in a script.

### Minor Changes

- `pnpm pack` and `pnpm publish` now warn when the tarball includes a `.env` or `.env.*` file that the `files` field of `package.json` does not list. Templates such as `.env.example` are not reported. List the file in `files` to publish it on purpose, or exclude it in `.npmignore` or `.gitignore` [#7826](https://github.com/pnpm/pnpm/issues/7826).

- `pnpm pack` now honors `--silent`, `--reporter=silent`, and `--loglevel=silent` to hide the tarball contents and summary. With `--json`, lifecycle script output and the final JSON output remain visible [#10297](https://github.com/pnpm/pnpm/issues/10297).

### Patch Changes

#### Installing packages

- Installing through a `pnpr` server now records the pnpmfile checksum in the lockfile, so a later `pnpm install --frozen-lockfile` accepts that lockfile [#14460](https://github.com/pnpm/pnpm/issues/14460). A frozen install through the pnpr server now fails if the pnpmfile changed. If the pnpmfile defines a `readPackage`, `afterAllResolved` or `preResolution` hook or custom resolvers, pnpm resolves dependencies locally and prints a warning that the pnpr server was not used.

  Installing through a `pnpr` server also links a workspace project at the directory its `publishConfig.directory` names. A server that does not forward the setting makes the install fail with `ERR_PNPM_PNPR_PUBLISH_DIRECTORY_MISMATCH`, so pnpm never writes a lockfile that points at the wrong directory. The server rejects a `publishConfig.directory` that points outside its project.

- Installing a git-hosted dependency that has to be built no longer fails when that dependency's own dependencies have build scripts nobody approved. pnpm skips those builds while preparing the dependency, as it does without `strictDepBuilds` [#9764](https://github.com/pnpm/pnpm/issues/9764).

- A git-hosted dependency that is a pnpm workspace with no committed lockfile is now detected as a pnpm project [#14011](https://github.com/pnpm/pnpm/issues/14011).

- `pnpm install --dev` and `pnpm fetch --dev` now install the optional dependencies of devDependencies, such as the platform binaries of Biome and oxlint. The project's own `optionalDependencies` are still skipped [#9678](https://github.com/pnpm/pnpm/issues/9678).

- `pnpm install --offline` and `pnpm add --offline` now resolve a version range to the newest matching version whose tarball is already in the store. They used to pick the newest version in the cached metadata and fail with `ERR_PNPM_NO_OFFLINE_TARBALL` when its tarball was missing [#10715](https://github.com/pnpm/pnpm/issues/10715).

- If an offline install fails because the registry metadata cache uses the layout from before pnpm 11.27 and 12.4, the error now names the older mirror on disk and explains that one online install repopulates the cache. The error also carries the `ERR_PNPM_NO_OFFLINE_META` code. `pnpm cache prune --help` now says that pnpm 11.26 and earlier, and pnpm 12.3 and earlier, depend on the directories it removes [#15656](https://github.com/pnpm/pnpm/issues/15656).

- Running `pnpm install` now refreshes dependencies when a package declared with a local `file:` directory changes its dependencies [#4623](https://github.com/pnpm/pnpm/issues/4623).

- A repeat `pnpm install` now keeps its fast up-to-date check when an override replaces a declared local `file:` dependency [#12892](https://github.com/pnpm/pnpm/issues/12892).

- `pnpm install` now removes an optional dependency from `node_modules` if its install script fails. Code that checks whether the package is installed no longer finds a package that cannot load [#8756](https://github.com/pnpm/pnpm/issues/8756).

- With `nodeLinker: hoisted`, `pnpm install` now restores a workspace project's `node_modules` after it was deleted. Before, the install printed "Already up to date" and left the project without the dependencies nested under it. On Windows, the install also no longer fails with "Access is denied" when another project's copy of a shared dependency links to the deleted directory.

- Under `nodeLinker: hoisted`, `pnpm install` now clears orphaned package directories that an interrupted or failed install leaves in a project's `node_modules`. A directory recorded by the previous install is removed, while an unrecorded directory is moved to `node_modules/.ignored`. A copy already in `.ignored` is never overwritten [#13676](https://github.com/pnpm/pnpm/issues/13676).

- Concurrent installs no longer fail when they replace the same stale hoisted dependency link. Virtual store cleanup now keeps the temporary lockfiles that concurrent installs write.

#### Resolving and linking dependencies

- `pnpm install` no longer aborts on a failed allocation of many gigabytes when peer dependency ranges combine overlapping `||` alternatives [#15867](https://github.com/pnpm/pnpm/issues/15867).

- `pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).

- An `npm:` alias written by `overrides` now stays in place when a change elsewhere makes pnpm re-resolve the aliased dependency. Before, pnpm could look up the alias name at the aliased version, which failed with `ERR_PNPM_NO_MATCHING_VERSION` or locked an unrelated package [#16309](https://github.com/pnpm/pnpm/issues/16309).

- A peer dependency no longer resolves to two different versions for one package. This happened when the package peer-depends on another package and on one of that package's peers, and it is installed deeper than a direct dependency of the package that provides them [#12098](https://github.com/pnpm/pnpm/issues/12098).

- An optional peer dependency is no longer resolved from another workspace project's package when the project provides one of that package's own peers at a version it rejects. This avoids bogus unmet peer errors [#13989](https://github.com/pnpm/pnpm/issues/13989).

- `pnpm dedupe` no longer changes the lockfile on every run when a nested peer dependency is provided through an npm alias [#15709](https://github.com/pnpm/pnpm/issues/15709).

- With `resolutionMode: time-based` and `minimumReleaseAge` both set, `pnpm install` no longer reports a subdependency as too new when only the time-based cutoff excludes it. Such subdependencies used to fail a strict install with `ERR_PNPM_NO_MATURE_MATCHING_VERSION`, or were added to `minimumReleaseAgeExclude` [#13569](https://github.com/pnpm/pnpm/issues/13569). A transitive dependency that has no matching version published before the time-based cutoff now resolves to the lowest matching version allowed by `minimumReleaseAge`. pnpm picks a version younger than `minimumReleaseAge` only if no older version matches [#16298](https://github.com/pnpm/pnpm/issues/16298).

- `pnpm install` retries registry metadata fetches that fail with a timeout, a dropped connection, or an interrupted response body before it applies `trustPolicy` or `minimumReleaseAge`. A transient fetch failure is not reported as `TRUST_DOWNGRADE` or `MINIMUM_RELEASE_AGE_VIOLATION` [#12031](https://github.com/pnpm/pnpm/issues/12031).

- pnpm's built-in package compatibility database no longer applies to a project's own manifest. A project named like a published package, such as `vue-loader`, no longer gains dependencies on `pnpm install` or `pnpm update`. User-configured `packageExtensions` still apply to project manifests [#11700](https://github.com/pnpm/pnpm/issues/11700).

- Packages in an external `virtualStoreDir` can resolve the project's direct dependencies selected by `hoistPattern`. Run `pnpm install --force` to repair an existing installation [#5652](https://github.com/pnpm/pnpm/issues/5652).

- `pnpm install` now links the executables of auto-installed peer dependencies into the workspace root's `node_modules/.bin`, including after a frozen-lockfile reinstall [#8511](https://github.com/pnpm/pnpm/issues/8511).

#### Lockfiles and frozen installs

- `pnpm install --frozen-lockfile` now works on a detached HEAD when `gitBranchLockfile` is enabled. The install reads the lockfiles of the local and remote-tracking branches that contain the checked-out commit. It still writes the shared `pnpm-lock.yaml` [#7672](https://github.com/pnpm/pnpm/issues/7672).

- `pnpm install --frozen-lockfile` now accepts a lockfile that has no importer entry for a workspace package without dependencies. Such a package added after the lockfile was written made the install fail with `ERR_PNPM_PACKAGE_MANAGER_NO_IMPORTER` [#15875](https://github.com/pnpm/pnpm/issues/15875).

- `pnpm install` now fails with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` when an importer references a dependency version that has no snapshot entry. Before, the install succeeded and left a `node_modules` symlink pointing at a missing virtual-store directory [#14764](https://github.com/pnpm/pnpm/issues/14764).

- `pnpm install` on CI now fails on an outdated lockfile when `preferFrozenLockfile` is explicitly set to `true`. Setting it to `true` used to let CI update the lockfile [#9072](https://github.com/pnpm/pnpm/pull/9072).

- With `gitBranchLockfile` enabled, each emoji or other character outside the Basic Multilingual Plane in a branch name now becomes `!!` in the lockfile name. Before, each such character became one `!`.

#### Workspaces and filtering

- `pnpm install` in a workspace with `sharedWorkspaceLockfile: false` now installs projects concurrently, up to `workspaceConcurrency` at a time [#14480](https://github.com/pnpm/pnpm/issues/14480). A project is resolved, fetched, and written to its virtual store without waiting for the workspace projects it depends on. It waits for them only before it links its dependencies and runs its lifecycle scripts, so its scripts still run after theirs. A project with a `preinstall` or `pnpm:devPreinstall` script, or with an injected or `file:` workspace dependency, waits for its workspace dependencies before it starts.

  The installs of the projects also share their package metadata, lockfile verification, and store caches, so they use less CPU and memory when several projects depend on the same packages. An install with a pnpmfile no longer starts an extra Node.js process when the pnpmfile has no `preResolution` hook.

- With `enableGlobalVirtualStore` and `sharedWorkspaceLockfile: false`, each project now keeps its current lockfile and its hidden hoisted dependencies in its own `node_modules/.pnpm`. Before, every project wrote them to the workspace root's `node_modules/.pnpm`, so each repeat install treated the other projects' packages as its own and relinked them [#14480](https://github.com/pnpm/pnpm/issues/14480).

- `pnpm rebuild`, `pnpm approve-builds`, and `pnpm ignored-builds` now work on the current project's `node_modules` when they run inside a project of a workspace with `sharedWorkspaceLockfile: false`. They used to read the workspace root's `node_modules`, so `pnpm rebuild` did not rebuild the project's dependencies and created a second virtual store at the workspace root [#9402](https://github.com/pnpm/pnpm/issues/9402).

- `pnpm install` no longer creates a `node_modules` symlink inside the `publishConfig.directory` of a workspace package linked with `linkDirectory`. A build tool that cleaned its output directory through that symlink deleted the files of the package's dependencies. `pnpm install` also removes a symlink that an earlier install left there [#16226](https://github.com/pnpm/pnpm/issues/16226). It also no longer fails with `ERR_PNPM_CMD_SHIM_RESOLVE_PATH` when such a package has a `bin` field and its `publishConfig.directory` does not exist yet.

- `pnpm install` no longer fails for an injected workspace dependency whose package publishes from a `publishConfig.directory` that its own `prepare` script builds. The injected copy now picks up that directory once `prepare` finishes building it. `pnpm install --frozen-lockfile` no longer reports the dependency as outdated while the directory has not been built yet [#7811](https://github.com/pnpm/pnpm/issues/7811).

- An in-place edit to the source of an injected workspace package now shows up in its injected copy, unless a build writes to that package or `packageImportMethod` is set. pnpm hardlinks such packages under the default import method [#4410](https://github.com/pnpm/pnpm/issues/4410). Scripts listed in `syncInjectedDepsAfterScripts` now update injected dependencies while they run, so a watcher on the injected package, such as a dev server, sees each change before the script exits.

- With `sharedWorkspaceLockfile: false`, an injected workspace package that has lifecycle scripts is now hard linked into the projects that depend on it. Before, pnpm left a plain copy, so later edits to the package did not reach those projects [#9828](https://github.com/pnpm/pnpm/issues/9828).

- `injectWorkspacePackages` now hard links a workspace dependency declared with a relative path, such as `workspace:../foo`, the same way it already does for `workspace:*` [#10446](https://github.com/pnpm/pnpm/issues/10446).

- Workspace discovery prunes dot-prefixed directories, so a `packages` pattern such as `**` no longer matches projects inside `.cache` and other hidden directories [#16250](https://github.com/pnpm/pnpm/issues/16250).

- `pnpm import` in a workspace now keeps the versions pinned by a `yarn.lock` inside a workspace project [#4385](https://github.com/pnpm/pnpm/issues/4385).

#### Store and caches

- Files imported from the store now follow the umask of the install that writes them. Installing with a umask of `077` no longer leaves imported files readable by the group and others [#3807](https://github.com/pnpm/pnpm/issues/3807).

- `pnpm install` keeps the owner, group, and mode of files already in a shared store, including `index.db`. New store files and directories inherit the store directory's group-write bit. When that directory is setgid, new files inherit its group. pnpm does not change a file's owner or group [#12765](https://github.com/pnpm/pnpm/issues/12765).

- When `pnpm install` repairs a store file that was modified through a hard link in `node_modules`, the repair now keeps the file's inode on Linux and macOS, so hard-linked copies in other projects are healed at the same time. On Windows the repair still replaces the file, so other projects are healed on their next install [#3445](https://github.com/pnpm/pnpm/issues/3445).

- `pnpm install` now reports a full store at once when writing package files fails. It no longer retries the tarball [#8581](https://github.com/pnpm/pnpm/issues/8581).

- pnpm now warns when it cannot hard link packages from an existing store in the pnpm home directory and falls back to a store on the project's filesystem. This can happen when the project is on another filesystem, such as a bind-mounted workspace in a container. The warning names both stores and suggests setting `storeDir` [#14505](https://github.com/pnpm/pnpm/issues/14505).

- The side-effects cache now restores the symlinks that a build script creates inside a package. A warm install used to replace each of them with a copy of its target [#12859](https://github.com/pnpm/pnpm/issues/12859).

  After upgrading, every package with a build script is built once more.

- The global virtual store and the side-effects cache now key built packages by the Node.js version that the root project's `devEngines.runtime` or `engines.runtime` pins. That is the Node.js their build scripts run with. A dependency that declares its own `engines.runtime` no longer changes the key for every other package.

- With `enableGlobalVirtualStore`, an install into a fresh `node_modules` no longer runs the build scripts of a dependency whose global virtual store slot an earlier install already built. `pnpm rebuild` still runs them [#14480](https://github.com/pnpm/pnpm/issues/14480).

- Concurrent installs that share a global virtual store now run a package's build in its shared slot one at a time. A failed build leaves the slot in place and marks it for the next install to rebuild [#15568](https://github.com/pnpm/pnpm/issues/15568).

- A warm `pnpm install` reuses on-disk package metadata for five minutes when the registry does not send an ETag. Registries that send an ETag, including the public npm registry, still revalidate with a conditional request. `pnpm update` still fetches current metadata [#13976](https://github.com/pnpm/pnpm/issues/13976).

- pnpm no longer revalidates cached registry metadata when the registry sends `Cache-Control: max-age=0`, `no-cache`, or `no-store`. It downloads the metadata again, so a version newly published to such a registry is visible on the next install [#13487](https://github.com/pnpm/pnpm/issues/13487).

- `pnpm install` honors `Cache-Control` for dependencies named with an `http:` or `https:` tarball URL. A fresh response is taken from the store with no request, and a stale one is revalidated with `If-None-Match` [#15648](https://github.com/pnpm/pnpm/issues/15648).

#### Patched dependencies

- `pnpm install` now repairs a `pnpm-lock.yaml` whose `(patch_hash=<hash>)` dependency paths disagree with its `patchedDependencies` map, including paths that lack the hash their patch calls for. Before, pnpm accepted such a lockfile as up to date and kept the old patched files. `pnpm install --frozen-lockfile` now fails on such a lockfile with `ERR_PNPM_INCONSISTENT_PATCH_HASH`. It fails with `ERR_PNPM_UNCHECKABLE_PATCH_HASH` when a patch hash in the lockfile is malformed, or when the lockfile lacks the package version or patch entry that the check needs [#15336](https://github.com/pnpm/pnpm/pull/15336).

- `pnpm install` with `nodeLinker: hoisted` now applies a patch once to each copy of a patched dependency in a workspace. Before, a copy that several workspace projects shared could receive the patch twice and end up with the patched content duplicated [#7565](https://github.com/pnpm/pnpm/issues/7565).

- `pnpm install` and `pnpm fetch` now fail with `ERR_PNPM_PATCH_NOT_FOUND` when a patch file listed in `patchedDependencies` does not exist [#5268](https://github.com/pnpm/pnpm/issues/5268).

- `engineStrict` now checks the patched `package.json` when a `patchedDependencies` entry changes `engines`. A patch that relaxes `engines.node` no longer fails the install against the published range [#9603](https://github.com/pnpm/pnpm/issues/9603).

- `pnpm patch` now applies the existing patch file to the edit directory of a git-hosted dependency, as it already does for packages from the registry [#9699](https://github.com/pnpm/pnpm/issues/9699).

#### Adding, updating, and removing dependencies

- `pnpm add <dir>` now warns when the added directory declares peer dependencies, as `pnpm link` does. The directory is saved as a `link:` dependency, and its peers are not resolved from the project that adds it. Use the `file:` protocol to have them resolved [#5523](https://github.com/pnpm/pnpm/issues/5523).

- `pnpm add --save-types` no longer adds a `@types/*` package whose resolved version is deprecated. DefinitelyTyped publishes such stubs for packages that ship their own types, such as `@types/typescript` for `typescript` [#15636](https://github.com/pnpm/pnpm/issues/15636).

- `pnpm version`, `pnpm add`, and `pnpm pkg set` keep JSON5 style when they update `package.json5`. ASCII identifier keys stay unquoted, strings keep JSON5 quotes, and indented files keep trailing commas [#15717](https://github.com/pnpm/pnpm/issues/15717).

#### Running scripts and commands

- `pnpm run` and `pnpm exec` no longer install dependencies automatically when the root `package.json` still keeps `overrides`, `packageExtensions`, `patchedDependencies`, or `ignoredOptionalDependencies` in its `pnpm` field. pnpm no longer reads that field, so the install rewrote the lockfile without those settings. The command now fails and asks to move the settings to `pnpm-workspace.yaml` [#16278](https://github.com/pnpm/pnpm/issues/16278).

- When `verifyDepsBeforeRun` triggers an install before a filtered `pnpm run` or `pnpm exec`, pnpm now installs only the selected projects and their dependencies. A later filtered command also installs a selected project that an earlier filtered install skipped [#11865](https://github.com/pnpm/pnpm/issues/11865).

- `pnpm -r run /regexp/` now honors the `tasks` `dependsOn` declared for each script the selector matches, like running the script by name does. Matched scripts that depend on each other run in order. Each matched script runs once [#15596](https://github.com/pnpm/pnpm/issues/15596).

- `pnpm run` exits with the code of a script that handles Ctrl+C and shuts down. A script that finished cleanly is not reported as a lifecycle failure. The commands after it in the same script still run [#9945](https://github.com/pnpm/pnpm/issues/9945).

- pnpm no longer hangs after a lifecycle script exits while a process it started in the background keeps the script's output open. pnpm stops reading that output one second after the script exits [#5730](https://github.com/pnpm/pnpm/issues/5730).

- `pnpm run` and lifecycle scripts use the configured `scriptShell`, including Git Bash on Windows, when `shellEmulator` is also enabled. `shellEmulator` still runs scripts when `scriptShell` is not set. Extra arguments passed to `pnpm run` are quoted for the shell that runs the script, so a Windows path stays intact [#14719](https://github.com/pnpm/pnpm/issues/14719).

- With `enableGlobalVirtualStore`, dependency build scripts now see the workspace root's `node_modules/.bin`, as they do with a local virtual store. A `postinstall` script that runs `node` finds the Node.js installed by `devEngines.runtime` and no longer fails with "command not found" on machines without a system Node.js [#15652](https://github.com/pnpm/pnpm/issues/15652). Dependency build scripts also see the bins of privately hoisted dependencies.

- Dependency install scripts now find the node-gyp bundled with pnpm when pnpm runs through a symlink, such as `node_modules/.bin/pnpm` or the `pnpm` that `npm install -g pnpm` links. They used to fail with `node-gyp: command not found` on macOS [#15694](https://github.com/pnpm/pnpm/issues/15694).

- `pnpm run` and lifecycle scripts now set `npm_config_node_gyp` to the bundled `node-gyp` entry point. Tools that read the variable resolve the same `node-gyp` pnpm builds with. An `npm_config_node_gyp` value the environment already sets is kept as is [#16270](https://github.com/pnpm/pnpm/issues/16270).

- Scripts now see the `npm_command` environment variable that npm sets. It holds `run-script` when the command runs a script, and the command's own name otherwise [#16265](https://github.com/pnpm/pnpm/issues/16265).

- Commands run from a POSIX shell through a dependency's own `node_modules/.bin`, such as `node_modules/vite/node_modules/.bin/esbuild`, no longer fail with `MODULE_NOT_FOUND` [#10189](https://github.com/pnpm/pnpm/issues/10189).

- `pnpx --version` and `pnpm dlx --version` now print the pnpm version. Other unknown options before the command are reported as errors. Before, pnpm tried to download a package named after the option [#16259](https://github.com/pnpm/pnpm/issues/16259).

- `pnpm dlx` now keeps the virtual store of its cached installs in `node_modules/.pnpm`, like every other install [#13955](https://github.com/pnpm/pnpm/issues/13955). `pnpm pack-app` now names the manifest of its runtime install directory `pnpm-pack-app-<target>`.

#### Publishing, packing, and deploying

- `pnpm pack` and `pnpm publish` now ship a file that the `files` field names even when another entry excludes the directory holding it. For example, `["**", "!dist", "dist/index.d.ts"]` ships `dist/index.d.ts` [#16213](https://github.com/pnpm/pnpm/issues/16213).

- `pnpm pack` prunes a directory that a `files` field exclusion names, such as `!**/test`, excluding the directory and its contents from the packed package [#15738](https://github.com/pnpm/pnpm/issues/15738).

- `pnpm publish` now waits at least 5 minutes for the registry to answer a publish request, like npm. This fixes "409 Conflict - Failed to save packument" errors when the registry is slow to answer [#11454](https://github.com/pnpm/pnpm/issues/11454).

- `pnpm deploy --prod` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the deployed project declares a `devEngines.runtime` with `onFail: download`. The runtime stays out of the deployed `node_modules` with the rest of the dev dependencies [#15703](https://github.com/pnpm/pnpm/issues/15703).

- `pnpm deploy` with a shared lockfile now copies workspace dependencies into the deploy directory, even when `packageImportMethod` is set to `hardlink`. Before, their files were hard-linked to the workspace sources, so editing a source file also changed the deployed copy [#12176](https://github.com/pnpm/pnpm/issues/12176).

- `pnpm deploy --legacy` no longer leaves broken links to nested local dependencies of workspace packages [#9575](https://github.com/pnpm/pnpm/issues/9575).

#### Configuration and pnpmfile hooks

- Every setting pnpm supports can now be set with `--config.<name>=<value>` on the command line, not only the ones whose command also carries a matching flag. Before, `pnpm install --config.frozen-lockfile=true` dropped the setting and rewrote `pnpm-lock.yaml` as though the install had not been frozen [#16276](https://github.com/pnpm/pnpm/issues/16276).

- Settings given on the command line, such as `--registry` and `--store-dir`, now take precedence over the values a pnpmfile `updateConfig` hook sets [#14063](https://github.com/pnpm/pnpm/issues/14063).

- `pnpm config set --location=project` and `pnpm config delete --location=project`, run from a package inside a workspace, now write settings that belong in `pnpm-workspace.yaml` to the workspace root's `pnpm-workspace.yaml`. Before, they created a new `pnpm-workspace.yaml` in the current package, which made that package the workspace root. Settings stored in `.npmrc` are still written to the current directory [#13757](https://github.com/pnpm/pnpm/issues/13757).

- pnpm now reads the workspace directory override from `PNPM_CONFIG_WORKSPACE_DIR`, like other settings. `NPM_CONFIG_WORKSPACE_DIR` still works as a fallback [#16275](https://github.com/pnpm/pnpm/issues/16275).

- pnpm now fails with `ERR_PNPM_AUTH_INVALID_BASE64` when a registry's `_password` in `.npmrc` is not valid base64. Before, it sent the value as the raw password. A `username` or `_password` left empty, for example by an unset environment variable, now supplies no credential [#16273](https://github.com/pnpm/pnpm/issues/16273).

- `proxy=false` now turns proxying off even when `HTTP_PROXY`, `HTTPS_PROXY`, or `ALL_PROXY` is set. pnpm no longer sends requests through a proxy named only in `ALL_PROXY`.

- `pnpm install` now runs the install hooks of a config dependency plugin's pnpmfile, including `readPackage`, `afterAllResolved`, and custom resolvers. Its pnpmfile is also counted in `pnpmfileChecksum`. Before, only the plugin's `updateConfig` hook ran, so a plugin could not change the resolved dependencies.

- A pnpmfile `fetchers` hook now runs once per package on a fresh install when it handles a resolution with a custom `type` or delegates a git-hosted one to the same subdirectory [#15584](https://github.com/pnpm/pnpm/issues/15584). These packages were fetched a second time for installation, so the installed files could come from a different archive than the one their dependencies were read from. The hook also no longer runs twice when a `resolvers` hook returns a tarball resolution without a manifest [#15025](https://github.com/pnpm/pnpm/issues/15025).

- `pnpm install` now re-fetches a package from a custom resolver when the `integrity` of its resolution changes, with or without `enableGlobalVirtualStore`. It used to update the lockfile but keep the old files in `node_modules` [#15670](https://github.com/pnpm/pnpm/issues/15670).

- `pnpm install` now rejects invalid results from a `readPackage` hook. A hook that returns a non-object value fails with `ERR_PNPM_BAD_READ_PACKAGE_HOOK_RESULT` [#15730](https://github.com/pnpm/pnpm/issues/15730). A hook that sets a dependency range to a value other than a string, such as `undefined`, fails with an error that names the dependency, the package and the pnpmfile. Delete the property to remove a dependency [#15705](https://github.com/pnpm/pnpm/issues/15705).

#### Global packages, pnpm versions, and runtimes

- `pnpm update --global` now reinstalls the global packages that pnpm 10 installed into the previous global directory, `<global-dir>/5`, so their commands are linked into the pnpm home `bin` directory again and `pnpm list --global` lists them. Once every package is migrated, pnpm deletes the previous directory and the commands pnpm 10 linked into the pnpm home [#11528](https://github.com/pnpm/pnpm/issues/11528).

- A signal sent to pnpm, such as `SIGTERM`, now reaches the pnpm that pnpm switches to because of `packageManager` or `devEngines.packageManager`, and the one that `pnpm with` runs. The signal used to be dropped, so scripts running under that pnpm never got to shut down [#9948](https://github.com/pnpm/pnpm/issues/9948).

- On arm64 musl Linux, such as Alpine on ARM, switching to a pinned pnpm older than 12 now runs the JavaScript `pnpm` package. The standalone executable of those versions crashed at startup on that platform [#10443](https://github.com/pnpm/pnpm/issues/10443).

- Global shims such as `node` now work when pnpm runs through a relative symlink, as with a Homebrew install. They were copies of that symlink and did not resolve from the global bin directory [#15691](https://github.com/pnpm/pnpm/issues/15691).

- `pnpm env remove --global` deletes Node.js versions that pnpm installed into its own store, including when another tool installed pnpm [#8357](https://github.com/pnpm/pnpm/issues/8357).

- `pnpm self-update` no longer suggests a downgrade when `minimumReleaseAge` holds back the registry's `latest` release. It now says that release is still within the cutoff [#12006](https://github.com/pnpm/pnpm/issues/12006).

#### Windows

- Interrupting a script with Ctrl+C on Windows no longer leaves the terminal stuck [#14860](https://github.com/pnpm/pnpm/issues/14860). A script that runs through a batch shim, as `vite dev` does through `vite.CMD`, made cmd.exe wait forever on its "Terminate batch job (Y/N)?" answer, and every following keystroke went to that prompt. pnpm now ends a cmd.exe script shell once it has sat for a second after the interrupt with nothing running under it. A script that takes longer to shut down is still waited for. A second Ctrl+C ends the script's shell at once.

- On Windows, `pnpm run` now passes the arguments after the script name to the script as typed. Before, `cmd` expanded `%VAR%` in them and backslashes arrived doubled. Line breaks still arrive as the two characters `\n`, because `cmd` cannot pass them. The command line pnpm prints for the script quotes the arguments the same way on every platform [#16257](https://github.com/pnpm/pnpm/issues/16257).

- The Windows `pnpm.exe` runs on a clean Windows install that does not have the Visual C++ Redistributable. It used to exit immediately on startup because that runtime was missing [#15723](https://github.com/pnpm/pnpm/issues/15723).

- On Windows, the `.cmd` command shims in `node_modules/.bin` now keep a `%` in the project path. Before, cmd.exe expanded it as a variable reference, so the command received a mangled `NODE_PATH` [#15716](https://github.com/pnpm/pnpm/issues/15716). Command shims also run tools whose paths contain non-ASCII characters [#6999](https://github.com/pnpm/pnpm/issues/6999), including the PowerShell shims in Windows PowerShell 5.1 [#16217](https://github.com/pnpm/pnpm/issues/16217).

- Bin shims in `node_modules/.bin` run from Cygwin on Windows again. The shims passed a `/cygdrive/c/...` path to the Windows `node` found on `PATH`, so Node.js failed with `Cannot find module 'C:\cygdrive\c\...'` [#12845](https://github.com/pnpm/pnpm/issues/12845).

- On Windows, installing pnpm with npm inside a project now writes `node_modules/.bin` shims that run `pnpm.exe`. A global install with `npm install --location=global` now gets the same shims as `npm install -g` [#15688](https://github.com/pnpm/pnpm/issues/15688).

- `pnpm install` no longer fails with `ERR_PNPM_WORKSPACE_INVALID_GLOB` on Windows for a wildcard pattern such as `plugins/*/*` in `pnpm-workspace.yaml` when the workspace is on a different drive than the pnpm cache or state directory [#16239](https://github.com/pnpm/pnpm/issues/16239).

- On Windows, `pnpm install` no longer skips a dependency's build script on a later install when the package ships an executable file and the script changes nothing inside the package directory [#15667](https://github.com/pnpm/pnpm/issues/15667).

- `pnpm setup` no longer writes the `pn.ps1`, `pnpx.ps1`, and `pnx.ps1` PowerShell wrappers. It also removes the ones an earlier setup wrote. PowerShell now runs `pn`, `pnpx`, and `pnx` through their `.cmd` wrappers, like `pnpm` itself. Before, these aliases failed with a "not digitally signed" error wherever the execution policy blocks unsigned scripts [#8444](https://github.com/pnpm/pnpm/issues/8444).

- `pnpm setup` on Windows no longer panics when an unrelated environment variable has a name containing a non-ASCII character. It skips that variable [#15684](https://github.com/pnpm/pnpm/issues/15684).

- On Windows, `pnpm setup` repairs the `PNPM_HOME` registry type left by older pnpm versions, even when the configured directory has not changed.

- On Windows, the `ERR_PNPM_BAD_ENV_FOUND` error of `pnpm setup` now shows the value `PNPM_HOME` is currently set to. Before, it showed the directory pnpm wanted to set.

- On Windows, pnpm expands nested `%VAR%` references in `PNPM_HOME` and the other directory environment variables it uses for its home, store, cache, state, and config directories. pnpm fails with an error when a `%VAR%` reference remains after expansion [#13236](https://github.com/pnpm/pnpm/issues/13236).

- On Windows, if the global bin directory is not in `PATH` and a `PATH` entry still contains an unexpanded variable such as `%PNPM_HOME%`, the error now names that entry. A variable referenced from the user `Path` must be set to a full path and stored as a plain string (`REG_SZ`) for the entry to expand [#5283](https://github.com/pnpm/pnpm/issues/5283).

#### Inspecting dependencies

- `pnpm audit` and `pnpm audit signatures` now fail with an error when the lockfile contains unresolvable dependency references [#13638](https://github.com/pnpm/pnpm/issues/13638).

- `pnpm licenses list` now reports the actual on-disk package locations when using `nodeLinker: hoisted` or `shamefully-hoist: true` [#8589](https://github.com/pnpm/pnpm/issues/8589). With `--json`, its `paths` array now includes every installed copy of a package, including hoisted copies and isolated installations with different peer dependencies.

- `pnpm root` now prints the configured `modulesDir`. It used to print `node_modules` regardless of the setting. A project's own `modulesDir` from `packageConfigs` is printed too [#9113](https://github.com/pnpm/pnpm/issues/9113).

#### Output and messages

- With the default and append-only reporters, installs with `--loglevel warn` or `--loglevel error` now print the full output of a failed install script. The output of successful scripts, including the root project's own install hooks, stays hidden. With `--loglevel warn`, pnpm also prints ignored build script warnings.

- When a dependency fails to resolve, the error now shows the cause. For example, a Node.js runtime download behind a proxy that re-signs TLS now reports `invalid peer certificate: UnknownIssuer` [#9556](https://github.com/pnpm/pnpm/issues/9556).

- When installing a git dependency over SSH fails with `Permission denied (publickey)`, pnpm suggests checking the loaded keys with `ssh-add -l`. Resolving an SSH URL that refuses the key also shows a local HTTPS rewrite that leaves the recorded URL alone [#13743](https://github.com/pnpm/pnpm/issues/13743).

- Lockfile verification now fails with `ERR_PNPM_TARBALL_URL_MISMATCH`, `ERR_PNPM_TARBALL_REVISION_MISMATCH`, or `ERR_PNPM_MISSING_NAMED_REGISTRY` when every rejected entry failed that check. These failures were reported as the generic `ERR_PNPM_LOCKFILE_RESOLUTION_VERIFICATION`.

- The lockfile verification error now suggests relaxing the policy that flagged an entry only if a fresh resolution still fails and you trust the affected packages. Errors from checks that no policy controls, such as a missing tarball integrity, no longer suggest relaxing a policy [#14411](https://github.com/pnpm/pnpm/issues/14411).

- `pnpm install` no longer prints an extra `Progress:` line after the progress line is marked `done` [#16184](https://github.com/pnpm/pnpm/issues/16184).
