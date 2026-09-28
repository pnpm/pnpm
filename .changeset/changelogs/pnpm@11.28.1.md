## 11.28.1

pnpm 11.28.1 makes `pnpm install` work in StackBlitz WebContainers, repairs lockfiles whose patch hashes disagree with `patchedDependencies`, and fixes many bugs in Windows command shims, injected workspace dependencies, and `pnpm deploy`.

### Patch Changes

#### Installing packages

- `pnpm install` now works in StackBlitz WebContainers. On projects without a lockfile, it used to fail with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls. It also failed with "this.db.exec is not a function", because `node:sqlite` there lacks `DatabaseSync.exec`. When `node:sqlite` cannot prepare statements either, pnpm stores the index in `index.fallback` [#15649](https://github.com/pnpm/pnpm/issues/15649).

- `pnpm install` now completes after downloading a Node.js runtime specified by `devEngines.runtime` when pnpm runs on Node.js 24.4.x [#14667](https://github.com/pnpm/pnpm/issues/14667).

- `pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).

- Installing a git-hosted dependency that has to be built no longer fails when that dependency's own dependencies have build scripts nobody approved. pnpm skips those builds while preparing the dependency, as it does without `strictDepBuilds` [#9764](https://github.com/pnpm/pnpm/issues/9764).

- pnpm now uses pnpm to prepare a git-hosted dependency that is a pnpm workspace without a committed lockfile. It used npm before, which could skip the dependency's build [#14011](https://github.com/pnpm/pnpm/issues/14011).

- When installing a git dependency over SSH fails with `Permission denied (publickey)`, pnpm suggests checking the loaded keys with `ssh-add -l`. Resolving an SSH URL that refuses the key also shows a local HTTPS rewrite that leaves the recorded URL alone [#13743](https://github.com/pnpm/pnpm/issues/13743).

- `pnpm install --dev` and `pnpm fetch --dev` now install the optional dependencies of devDependencies, such as the platform binaries of Biome and oxlint. The project's own `optionalDependencies` are still skipped [#9678](https://github.com/pnpm/pnpm/issues/9678).

- `pnpm install --frozen-lockfile` now works on a detached HEAD when `gitBranchLockfile` is enabled. The install now reads the lockfiles of the local and remote-tracking branches that contain the checked-out commit. It still writes the shared `pnpm-lock.yaml` [#7672](https://github.com/pnpm/pnpm/issues/7672).

- `pnpm install` on CI now fails on an outdated lockfile when `preferFrozenLockfile` is explicitly set to `true`. Setting it to `true` used to let CI update the lockfile [#9072](https://github.com/pnpm/pnpm/pull/9072).

- `pnpm install` now fails with `ERR_PNPM_IGNORED_BUILDS` on a repeat install when `strictDepBuilds` is on and a dependency's build is still undecided. A repeat install against an existing `node_modules` reported success where a fresh install failed [#10450](https://github.com/pnpm/pnpm/issues/10450).

- `pnpm install` now removes an optional dependency from `node_modules` if its install script fails. Code that checks whether the package is installed no longer finds a package that cannot load [#8756](https://github.com/pnpm/pnpm/issues/8756).

- `pnpm install --offline` and `pnpm add --offline` now resolve a version range to the newest matching version whose tarball is already in the store. They used to pick the newest version in the cached metadata and fail with `ERR_PNPM_NO_OFFLINE_TARBALL` when its tarball was missing [#10715](https://github.com/pnpm/pnpm/issues/10715).

- If an offline install fails because the registry metadata cache uses the layout from before pnpm 11.27 and 12.4, the error now names the older mirror on disk and explains that one online install repopulates the cache [#15656](https://github.com/pnpm/pnpm/issues/15656).

- `pnpm install` now fails right away when writing package files fails because the store is full. It no longer retries the tarball download first [#8581](https://github.com/pnpm/pnpm/issues/8581).

- With `nodeLinker: hoisted`, `pnpm install` now restores a workspace project's `node_modules` after it was deleted. Before, the install printed "Already up to date" and left the project without the dependencies nested under it.

- Under `nodeLinker: hoisted`, `pnpm install` now clears orphaned package directories that an interrupted or failed install leaves in a project's `node_modules`. A directory recorded by the previous install is removed, while an unrecorded directory is moved to `node_modules/.ignored`. A copy already in `.ignored` is never overwritten [#13676](https://github.com/pnpm/pnpm/issues/13676).

- Packages in an external `virtualStoreDir` can resolve the project's direct dependencies selected by `hoistPattern`. Run `pnpm install --force` to repair an existing installation [#5652](https://github.com/pnpm/pnpm/issues/5652).

- A repeat install now keeps the fast path when a declared local file dependency is replaced by an override [#12892](https://github.com/pnpm/pnpm/issues/12892).

#### Store, build cache, and global virtual store

- Files imported from the store now follow the umask of the install that writes them. Installing with a umask of `077` no longer leaves imported files readable by the group and others [#3807](https://github.com/pnpm/pnpm/issues/3807).

- With the global virtual store, `pnpm rebuild` no longer modifies packages shared with projects that have not approved their build scripts [#12302](https://github.com/pnpm/pnpm/issues/12302).

- The side-effects cache now restores the symlinks that a build script creates inside a package. A warm install used to replace each of them with a copy of its target [#12859](https://github.com/pnpm/pnpm/issues/12859).

  After upgrading, every package with a build script is built once more.

- `pnpm install` now restores cached build artifacts when reinstalling a workspace that uses separate lockfiles [#12942](https://github.com/pnpm/pnpm/issues/12942).

- The global virtual store and the side-effects cache now key built packages by the Node.js version that the root project's `devEngines.runtime` or `engines.runtime` pins. That is the Node.js their build scripts run with. A dependency that declares its own `engines.runtime` no longer changes the key for every other package.

- Concurrent installs that share a global virtual store now run a package's build in its shared slot one at a time. A failed build leaves the slot in place and marks it for the next install to rebuild [#15568](https://github.com/pnpm/pnpm/issues/15568).

- Concurrent installs no longer fail when they replace the same stale hoisted dependency link. Virtual store cleanup now keeps the temporary lockfiles that concurrent installs are still writing.

- `pnpm install` keeps the owner, group, and mode of files already in a shared store, including `index.db`. New store files and directories inherit the store directory's group-write bit. When that directory is setgid, new files inherit its group. pnpm does not change a file's owner or group [#12765](https://github.com/pnpm/pnpm/issues/12765).

- When `pnpm install` repairs a store file that was modified through a hard link in `node_modules`, the repair now keeps the file's inode on Linux and macOS. Hard-linked copies in other projects are healed at the same time. Before, only the project running the install received the restored content. On Windows the repair still replaces the file, so other projects are healed on their next install [#3445](https://github.com/pnpm/pnpm/issues/3445).

- A tarball whose integrity pnpm computed during download is now found in the store on the next install. Before, that install downloaded the tarball again once the lockfile recorded the integrity [#12562](https://github.com/pnpm/pnpm/issues/12562).

- pnpm now warns when it cannot hard link packages from an existing store in the pnpm home directory and uses a store on the project's filesystem. This can happen when the project is on another filesystem, such as a bind-mounted workspace in a container. The warning names both stores and suggests setting `storeDir` [#14505](https://github.com/pnpm/pnpm/issues/14505).

#### Resolving and linking dependencies

- A peer dependency no longer resolves to two different versions for one package. This happened when the package peer-depends on another package and on one of that package's peers, and it is installed deeper than a direct dependency of the package that provides them [#12098](https://github.com/pnpm/pnpm/issues/12098).

- An optional peer dependency is no longer resolved from another workspace project's package when the project provides one of that package's own peers at a version it rejects. This avoids bogus unmet peer errors [#13989](https://github.com/pnpm/pnpm/issues/13989).

- pnpm no longer reports unmet peer dependency warnings for aliased `npm:` peer ranges that a tarball dependency satisfies [#11126](https://github.com/pnpm/pnpm/issues/11126).

- `pnpm install` now links the executables of auto-installed peer dependencies into the workspace root's `node_modules/.bin`, including after a frozen-lockfile reinstall [#8511](https://github.com/pnpm/pnpm/issues/8511).

- With `resolutionMode: time-based` and `minimumReleaseAge` both set, `pnpm install` no longer reports a subdependency as too new when only the time-based cutoff excludes it. Such subdependencies used to fail a strict install with `ERR_PNPM_NO_MATURE_MATCHING_VERSION`, or were added to `minimumReleaseAgeExclude` [#13569](https://github.com/pnpm/pnpm/issues/13569).

  A transitive dependency that has no matching version published before the time-based cutoff now resolves to the lowest matching version allowed by `minimumReleaseAge`. pnpm picks a version younger than `minimumReleaseAge` only if no older version matches [#16298](https://github.com/pnpm/pnpm/issues/16298).

- With `minimumReleaseAge` set, re-resolving the lockfile no longer rewrites the `peerDependencies` recorded for a package whose version did not change. This happened when the registry metadata of a package differed from the `package.json` in its tarball [#13988](https://github.com/pnpm/pnpm/issues/13988).

- pnpm no longer revalidates cached registry metadata when the registry sends `Cache-Control: max-age=0`, `no-cache`, or `no-store`. It downloads the metadata again, so a version newly published to such a registry is visible on the next install [#13487](https://github.com/pnpm/pnpm/issues/13487).

- pnpm's built-in package compatibility database no longer applies to a project's own manifest. A project named like a published package, such as `vue-loader`, no longer gains dependencies on `pnpm install` or `pnpm update`. User-configured `packageExtensions` still apply to project manifests [#11700](https://github.com/pnpm/pnpm/issues/11700).

- Relative local tarball paths in `pnpm.overrides` without an explicit `file:` prefix are now rebased correctly for workspace packages [#11131](https://github.com/pnpm/pnpm/issues/11131).

- pnpm no longer reports `pnpm-lock.yaml` as broken when a project depends on a package named `constructor`. A `__proto__` key in the lockfile is now kept as a plain entry when pnpm reads or writes the lockfile. It no longer replaces the prototype of the objects pnpm builds from it [#11028](https://github.com/pnpm/pnpm/issues/11028).

- With `nodeLinker: pnp`, a workspace package can now require another workspace package it depends on [#3567](https://github.com/pnpm/pnpm/issues/3567). On Windows, workspace dependency paths in the generated `.pnp.cjs` now use forward slashes.

#### Performance

- pnpm now uses less memory when installing a package whose archive is larger than 64 MiB unpacked, and when installing a runtime from a zip archive, such as Node.js on Windows, Deno, or Bun [#14164](https://github.com/pnpm/pnpm/issues/14164).

- A fresh install reusing a warm global virtual store skips reimporting packages whose target directory is already complete [#11112](https://github.com/pnpm/pnpm/issues/11112).

- A warm `pnpm install` reuses on-disk package metadata for five minutes when the registry does not send an ETag. Registries that send an ETag, including the public npm registry, still revalidate with a conditional request. `pnpm update` still fetches current metadata [#13976](https://github.com/pnpm/pnpm/issues/13976).

#### Patched dependencies

- `pnpm install` now repairs a `pnpm-lock.yaml` whose `(patch_hash=<hash>)` dependency paths disagree with its `patchedDependencies` map, including paths that lack the hash their patch calls for. pnpm previously accepted such a lockfile as up to date and kept the old patched files. `pnpm install --frozen-lockfile` now fails on such a lockfile with `ERR_PNPM_INCONSISTENT_PATCH_HASH`. It fails with `ERR_PNPM_UNCHECKABLE_PATCH_HASH` when a patch hash in the lockfile is malformed, or when the lockfile lacks the package version or patch entry that the check needs [#15336](https://github.com/pnpm/pnpm/pull/15336).

- `pnpm install` now fails with `ERR_PNPM_PATCH_NOT_FOUND` when a patch file listed in `patchedDependencies` does not exist. It used to fail with a raw `ENOENT` error and a stack trace [#5268](https://github.com/pnpm/pnpm/issues/5268).

- pnpm now fails with `ERR_PNPM_INVALID_PATCHED_DEPENDENCY` when `patchedDependencies` has an invalid shape or contains a non-string value.

- `engineStrict` now checks the patched `package.json` when a `patchedDependencies` entry changes `engines`. A patch that relaxes `engines.node` no longer fails the install against the published range [#9603](https://github.com/pnpm/pnpm/issues/9603).

- `pnpm patch` now applies the existing patch file to the edit directory of a git-hosted dependency, as it already does for packages from the registry [#9699](https://github.com/pnpm/pnpm/issues/9699).

- `pnpm patch-commit` now fails with an error when `git` cannot be found in `PATH`. It previously reported that no changes were found [#8666](https://github.com/pnpm/pnpm/issues/8666).

#### Workspaces and filtering

- `pnpm install` refreshes injected copies of workspace packages when source projects are rebuilt. Injected copies previously stayed stale until `pnpm install --force` [#4407](https://github.com/pnpm/pnpm/issues/4407).

- Scripts listed in `syncInjectedDepsAfterScripts` now update injected dependencies while they run. A watcher on the injected package, such as a dev server, sees each change before the script exits [#4410](https://github.com/pnpm/pnpm/issues/4410).

- `pnpm install` no longer fails for an injected workspace dependency whose package publishes from a `publishConfig.directory` that its own `prepare` script builds. The injected copy now picks up that directory once `prepare` finishes building it. `pnpm install --frozen-lockfile` no longer reports the dependency as outdated while the directory has not been built yet [#7811](https://github.com/pnpm/pnpm/issues/7811).

- With `sharedWorkspaceLockfile: false`, an injected workspace package that has lifecycle scripts is now hard linked into the projects that depend on it. Before, pnpm left a plain copy, so later edits to the package did not reach those projects [#9828](https://github.com/pnpm/pnpm/issues/9828).

- `injectWorkspacePackages` now hard links a workspace dependency declared with a relative path, such as `workspace:../foo`, the same way it already does for `workspace:*` [#10446](https://github.com/pnpm/pnpm/issues/10446).

- `pnpm install` no longer creates a `node_modules` symlink inside the `publishConfig.directory` of a workspace package linked with `linkDirectory`. A build tool that cleaned its output directory through that symlink deleted the files of the package's dependencies. `pnpm install` also removes a symlink that an earlier install left there [#16226](https://github.com/pnpm/pnpm/issues/16226).

- When `verifyDepsBeforeRun` triggers an install before a filtered `pnpm run` or `pnpm exec`, pnpm now installs only the selected projects and their dependencies. A later filtered command also installs a selected project that an earlier filtered install skipped [#11865](https://github.com/pnpm/pnpm/issues/11865).

- `pnpm import` in a workspace now keeps the versions pinned by a `yarn.lock` inside a workspace project [#4385](https://github.com/pnpm/pnpm/issues/4385).

- Installing with `pnprServer` set now records the pnpmfile checksum in the lockfile, so a later `pnpm install --frozen-lockfile` accepts that lockfile. A frozen install through the pnpr server now fails if the pnpmfile changed. If the pnpmfile defines a `readPackage`, `afterAllResolved` or `preResolution` hook or custom resolvers, pnpm resolves dependencies locally and prints a warning that the pnpr server was not used [#14460](https://github.com/pnpm/pnpm/issues/14460).

  Installing through a pnpr server now links a workspace project at the directory its `publishConfig.directory` names. An install that resolves through a server which does not forward the setting fails with `ERR_PNPM_PNPR_PUBLISH_DIRECTORY_MISMATCH`. The server rejects a `publishConfig.directory` that points outside its project.

#### Adding, updating, and removing dependencies

- With `autoInstallPeers`, `pnpm add` and `pnpm remove` in a workspace project keep the locked version of a peer dependency the project declares. In a workspace where another project depended on a different version of that package, the peer could switch to that version [#11225](https://github.com/pnpm/pnpm/issues/11225).

- Adding a dependency now keeps unrelated transitive dependencies on their locked versions [#11456](https://github.com/pnpm/pnpm/issues/11456).

- `pnpm update --recursive <pkg>` no longer changes the version of a peer dependency that another workspace project installs automatically. Such a peer could move to a version outside the range the project declares, for example to React 19 in a project that declares `react: ^18.3.1` [#14928](https://github.com/pnpm/pnpm/issues/14928).

- `pnpm update package@beta` on a dependency declared as `catalog:` now updates the catalog entry and keeps `catalog:` in `package.json`. Before, pnpm wrote the resolved specifier to `package.json` [#13399](https://github.com/pnpm/pnpm/issues/13399).

- `pnpm update` now applies an override that references a catalog with the catalog's new value when the update bumps that catalog entry. Before, the packages the override targets kept the old version in the lockfile [#12159](https://github.com/pnpm/pnpm/issues/12159).

- `pnpm add <dir>` now warns when the added directory declares peer dependencies, as `pnpm link` does. The directory is saved as a `link:` dependency, and its peers are not resolved from the project that adds it. Use the `file:` protocol to have them resolved [#5523](https://github.com/pnpm/pnpm/issues/5523).

#### Running scripts and commands

- `pnpm run` and `pnpm exec` no longer install dependencies automatically when the root `package.json` still keeps `overrides`, `packageExtensions`, `patchedDependencies`, or `ignoredOptionalDependencies` in its `pnpm` field. pnpm no longer reads that field, so the install rewrote the lockfile without those settings. The command now fails and asks to move the settings to `pnpm-workspace.yaml` [#16278](https://github.com/pnpm/pnpm/issues/16278).

- `pnpm run` no longer reinstalls dependencies when a `node_modules` directory installed outside CI is used with `CI=true`, or the other way around [#12337](https://github.com/pnpm/pnpm/issues/12337).

- pnpm no longer hangs after a lifecycle script exits while a process it started in the background keeps the script's output open. pnpm stops reading that output one second after the script exits [#5730](https://github.com/pnpm/pnpm/issues/5730).

- A signal sent to pnpm, such as `SIGTERM`, now reaches the pnpm that pnpm switches to because of `packageManager` or `devEngines.packageManager`, and the one that `pnpm with` runs. The signal used to be dropped, so scripts running under that pnpm never got to shut down [#9948](https://github.com/pnpm/pnpm/issues/9948).

- `pnpm run` exits with the code of a script that handles Ctrl+C and shuts down. A script that finished cleanly is not reported as a lifecycle failure. The commands after it in the same script still run [#9945](https://github.com/pnpm/pnpm/issues/9945).

- `pnpm run` and lifecycle scripts use the configured `scriptShell`, including Git Bash on Windows, when `shellEmulator` is also enabled. `shellEmulator` still runs scripts when `scriptShell` is not set. Extra arguments passed to `pnpm run` are quoted for the shell that runs the script, so a Windows path stays intact [#14719](https://github.com/pnpm/pnpm/issues/14719).

- `pnpm -r run /regexp/` now honors the `tasks` `dependsOn` declared for each script the selector matches, like running the script by name does. Matched scripts that depend on each other run in order. Each matched script runs once [#15596](https://github.com/pnpm/pnpm/issues/15596).

- Commands run from a POSIX shell through a dependency's own `node_modules/.bin`, such as `node_modules/vite/node_modules/.bin/esbuild`, no longer fail with `MODULE_NOT_FOUND` [#10189](https://github.com/pnpm/pnpm/issues/10189).

- `pnpm rebuild` with `nodeLinker: hoisted` no longer puts one package's parent `node_modules/.bin` directories on the `PATH` of the packages it builds after it.

#### Publishing, packing, and deploying

- `pnpm publish` now waits at least 5 minutes for the registry to answer a publish request, like npm. This fixes "409 Conflict - Failed to save packument" errors when the registry is slow to answer [#11454](https://github.com/pnpm/pnpm/issues/11454).

- `pnpm deploy` with a shared lockfile now copies workspace dependencies into the deploy directory, even when `packageImportMethod` is set to `hardlink`. Previously, their files were hard-linked to the workspace sources, so editing a source file also changed the deployed copy [#12176](https://github.com/pnpm/pnpm/issues/12176).

- `pnpm deploy --prod` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the deployed project declares a `devEngines.runtime` with `onFail: download`. The runtime stays out of the deployed `node_modules` with the rest of the dev dependencies [#15703](https://github.com/pnpm/pnpm/issues/15703).

- `pnpm deploy --legacy` no longer leaves broken links to nested local dependencies of workspace packages [#9575](https://github.com/pnpm/pnpm/issues/9575).

- `pnpm deploy` no longer creates extra directories inside the deploy target and workspace projects when using a relative deploy path [#10981](https://github.com/pnpm/pnpm/issues/10981).

#### Manifests and configuration files

- Settings given on the command line, such as `--registry` and `--store-dir`, now take precedence over the values a pnpmfile `updateConfig` hook sets [#14063](https://github.com/pnpm/pnpm/issues/14063).

- An `updateConfig` hook that returns `registriesByScope` without the `default` or `@jsr` entry no longer crashes the install with `Invalid URL`. A missing `default` keeps the configured `registry`, and a missing `@jsr` falls back to the built-in JSR registry [#15619](https://github.com/pnpm/pnpm/issues/15619).

  The hook's `registry` and the `default` entry of its `registriesByScope` now set one default registry, which installs, `pnpm publish`, and `pnpm login` all use. If a hook changes both, `registry` wins. A route that is not a string fails with `ERR_PNPM_INVALID_UPDATE_CONFIG_RESULT`.

- An `updateConfig` hook in `.pnpmfile.cjs` has to use `config.registriesByScope`, `config.registriesByPrefix`, and `config.registryOptionsByUrl`. These registry lookups were renamed in pnpm 11.23.0 from `config.registries`, `config.namedRegistries`, and `config.registryOptions` [#15620](https://github.com/pnpm/pnpm/issues/15620).

- An async `updateConfig` hook that resolves to `undefined` now fails with `ERR_PNPM_CONFIG_IS_UNDEFINED`, as a synchronous hook that returns `undefined` already did.

- `pnpm config set --location=project` and `pnpm config delete --location=project`, run from a package inside a workspace, now write settings that belong in `pnpm-workspace.yaml` to the workspace root's `pnpm-workspace.yaml`. Before, they created a new `pnpm-workspace.yaml` in the current package, which made that package the workspace root. Settings stored in `.npmrc` are still written to the current directory [#13757](https://github.com/pnpm/pnpm/issues/13757).

- pnpm now reads the workspace directory override from `PNPM_CONFIG_WORKSPACE_DIR`, like other settings. `NPM_CONFIG_WORKSPACE_DIR` still works as a fallback [#16275](https://github.com/pnpm/pnpm/issues/16275).

- A `${VAR}` placeholder in `.npmrc` or `pnpm-workspace.yaml` whose name matches a built-in object property, such as `${toString}`, is now treated as an unset variable. It used to be replaced with the source text of a JavaScript function.

- pnpm now rejects a falsy non-array `packages` field in `pnpm-workspace.yaml`, such as `packages: false`, with an error. It used to treat the field as omitted.

#### Global packages, pnpm versions, and runtimes

- `pnpm update --global` now reinstalls the global packages that pnpm 10 installed into the previous global directory, `<global-dir>/5`, so their commands are linked into the pnpm home `bin` directory again and `pnpm list --global` lists them. Once every package is migrated, pnpm deletes the previous directory and the commands pnpm 10 linked into the pnpm home [#11528](https://github.com/pnpm/pnpm/issues/11528).

- `pnpm env remove --global` deletes Node.js versions that pnpm installed into its own store, including when another tool installed pnpm [#8357](https://github.com/pnpm/pnpm/issues/8357).

- `pnpm self-update` no longer suggests a downgrade when `minimumReleaseAge` holds back the registry's `latest` release. It now says that release is still within the cutoff [#12006](https://github.com/pnpm/pnpm/issues/12006).

- `@pnpm/exe` no longer ships a binary for arm64 musl Linux, such as Alpine on ARM. The published binary crashed with a segmentation fault at startup. Installing `@pnpm/exe` on that platform now fails with an error that suggests `npm install -g pnpm` or pnpm 12 [#10443](https://github.com/pnpm/pnpm/issues/10443).

- The macOS and Linux release archives no longer include Windows-only files, such as `node-gyp.cmd` and the `fastlist` executables [#11352](https://github.com/pnpm/pnpm/issues/11352).

#### Windows and WSL

- On Windows, `pnpm run` now passes the arguments after the script name to the script as typed. Before, `cmd` expanded `%VAR%` in them and backslashes arrived doubled. Line breaks still arrive as the two characters `\n`, because `cmd` cannot pass them. The command line pnpm prints for the script quotes the arguments the same way on every platform [#16257](https://github.com/pnpm/pnpm/issues/16257).

- On Windows, command shims now run tools whose paths contain non-ASCII characters [#6999](https://github.com/pnpm/pnpm/issues/6999). The PowerShell shims do so in Windows PowerShell 5.1 too [#16217](https://github.com/pnpm/pnpm/issues/16217).

- On Windows, the `.cmd` command shims in `node_modules/.bin` now keep a `%` in the project path. Before, cmd.exe expanded it as a variable reference, so the command received a mangled `NODE_PATH` [#15716](https://github.com/pnpm/pnpm/issues/15716).

- Bin shims in `node_modules/.bin` run from Cygwin on Windows again. The shims passed a `/cygdrive/c/...` path to the Windows `node` found on `PATH`, so Node.js failed with `Cannot find module 'C:\cygdrive\c\...'` [#12845](https://github.com/pnpm/pnpm/issues/12845).

- `pnpm setup` no longer writes the `pn.ps1`, `pnpx.ps1`, and `pnx.ps1` PowerShell wrappers. It also removes the ones an earlier setup wrote. PowerShell now runs `pn`, `pnpx`, and `pnx` through their `.cmd` wrappers, like `pnpm` itself. Before, these aliases failed with a "not digitally signed" error wherever the execution policy blocks unsigned scripts [#8444](https://github.com/pnpm/pnpm/issues/8444).

- On Windows, globally installed `@pnpm/exe` commands now run in the invoking PowerShell console and return their exit status [#6503](https://github.com/pnpm/pnpm/issues/6503).

- On Windows, installing `@pnpm/exe` with npm inside a project now writes `node_modules/.bin` shims that run the standalone executable [#15688](https://github.com/pnpm/pnpm/issues/15688).

- On Windows, `pnpm env use -g` and `pnpm add -g node@runtime:<version>` now replace a `node.exe` in the global bin directory that is a broken symlink. Previously they failed with `ENOENT` [#5411](https://github.com/pnpm/pnpm/issues/5411).

- On Windows, pnpm expands nested `%VAR%` references in `PNPM_HOME` and the other directory environment variables it uses for its home, store, cache, state, and config directories. pnpm fails with an error when a `%VAR%` reference remains after expansion [#13236](https://github.com/pnpm/pnpm/issues/13236).

- On Windows, if the global bin directory is not in `PATH` and a `PATH` entry still contains an unexpanded variable such as `%PNPM_HOME%`, the error now names that entry. A variable referenced from the user `Path` must be set to a full path and stored as a plain string (`REG_SZ`) for the entry to expand [#5283](https://github.com/pnpm/pnpm/issues/5283).

- On Windows, `pnpm setup` no longer garbles non-ASCII characters in existing `Path` entries [#6346](https://github.com/pnpm/pnpm/issues/6346).

- On Windows, `pnpm setup` repairs the `PNPM_HOME` registry type left by older pnpm versions, even when the configured directory has not changed.

- On Windows, the `ERR_PNPM_BAD_ENV_FOUND` error of `pnpm setup` now shows the value `PNPM_HOME` is currently set to. It used to show the directory pnpm wanted to set.

- On Windows, `pnpm install` no longer skips a dependency's build script on a later install when the script changes nothing inside the package directory [#15667](https://github.com/pnpm/pnpm/issues/15667).

- On Windows, pnpm now retries writing the workspace state file while another process, such as an antivirus scanner, briefly holds it open [#14550](https://github.com/pnpm/pnpm/issues/14550).

#### Inspecting dependencies

- `pnpm licenses list` now reports the actual on-disk package locations when using `nodeLinker: hoisted` or `shamefully-hoist: true` [#8589](https://github.com/pnpm/pnpm/issues/8589). `pnpm licenses list --json` now includes every installed copy of a package in its `paths` array, including hoisted copies and isolated installations with different peer dependencies. Its paths also exist on disk when the isolated linker uses a custom `modulesDir`.

- `pnpm list` now shows the correct path of a `link:` dependency that points to a directory on another drive on Windows. The path used to be appended to the project directory, such as `C:\project\D:\lib`, and `pnpm list --long` could not show the package's details [#10362](https://github.com/pnpm/pnpm/issues/10362).

- `pnpm root` now prints the configured `modulesDir`. It used to print `node_modules` regardless of the setting. A project's own `modulesDir` from `packageConfigs` is printed too [#9113](https://github.com/pnpm/pnpm/issues/9113).

- `pnpm audit` and `pnpm audit signatures` now fail with an error when the lockfile contains unresolvable dependency references [#13638](https://github.com/pnpm/pnpm/issues/13638).

#### Output and messages

- With the default and append-only reporters, installs with `--loglevel warn` or `--loglevel error` now print the full output of a failed install script. The output of successful scripts, including the root project's own install hooks, stays hidden. With `--loglevel warn`, pnpm also prints ignored build script warnings.

- The lockfile verification error now suggests relaxing the policy that flagged an entry only if a fresh resolution still fails and you trust the affected packages. Errors from checks that no policy controls, such as a missing tarball integrity, no longer suggest relaxing a policy [#14411](https://github.com/pnpm/pnpm/issues/14411).

- `pnpm run --recursive` now prints GitLab CI collapsible sections that GitLab recognizes. The section markers used to appear as raw text in the job log.

- Local tarball dependencies using the file protocol are no longer counted as downloaded in the progress banner [#1103](https://github.com/pnpm/pnpm/issues/1103).
