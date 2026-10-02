## 12.9.0

This release runs pnpm in StackBlitz WebContainers, adds a per-registry `networkConcurrency` setting, and records every installed project in the store. It also carries a security fix for `pnpm login`.

### Minor Changes

- pnpm now automatically uses WebAssembly in StackBlitz WebContainers, including when installation scripts are disabled. Native installations continue to use the native executable when installation scripts are enabled.

- A `registries` entry can now set `networkConcurrency`, the most requests pnpm keeps in flight to that registry's origin. Requests to other registries keep the overall limit. The setting may live in `pnpm-workspace.yaml` or the global `config.yaml`.

  ```yaml
  registries:
    https://npm.corp.example.com/:
      scopes: ["@acme"]
      networkConcurrency: 4
  ```

- `pnpm install` now records every project it installs in the store's `projects` directory, as a symlink to the project directory. A `--frozen-store` install without the global virtual store still records nothing. Only projects that used the global virtual store were recorded before [#6929](https://github.com/pnpm/pnpm/issues/6929).

### Patch Changes

- `pnpm login` no longer forwards credentials in its request body to another origin during redirects.

#### Installing packages

- Fixed `pnpm install` failing on Android with `ERR_PNPM_STORE_DIR_ACQUIRE_OPERATION_LOCK` [#16508](https://github.com/pnpm/pnpm/issues/16508).

- `pnpm install --frozen-lockfile` again succeeds when a workspace project recorded in `pnpm-lock.yaml` has no directory, such as a project left out of a Docker build context. It still fails if the project's directory exists without a `package.json` [#16453](https://github.com/pnpm/pnpm/issues/16453).

- `pnpm install --frozen-lockfile` no longer requires a `pnpm-lock.yaml` in a project that has no dependencies. It also succeeds when `pnpm-lock.yaml` records only the pinned pnpm version, as other commands write it when they run before the first install [#16477](https://github.com/pnpm/pnpm/issues/16477).

- Fixed `pnpm install --frozen-lockfile` rejecting a fresh lockfile when an injected workspace dependency has an optional peer supplied by another workspace project [#16428](https://github.com/pnpm/pnpm/issues/16428).

- With `nodeLinker: hoisted`, a filtered install now keeps the packages of the workspace projects an earlier install put in `node_modules`. This also covers the install that `pnpm --filter <selector> run` and `pnpm --filter <selector> exec` start before the command. Before, these installs removed every package that only the unselected projects needed [#16483](https://github.com/pnpm/pnpm/issues/16483).

- `pnpm install` with `nodeLinker: hoisted` now refreshes directories supplied by custom fetchers when reinstalling. pnpm also keeps the symlinks inside those directories.

- With `enableGlobalVirtualStore` on, scripts can run entry points that a CommonJS require hook loads again, such as `ts-node index.ts`. They failed with `ERR_UNKNOWN_FILE_EXTENSION` on Node.js versions without built-in TypeScript support [#16436](https://github.com/pnpm/pnpm/issues/16436).

- pnpm now keeps each project's current lockfile and hidden hoisted dependencies in its own `node_modules/.pnpm` when `virtualStoreDir` points at a shared global virtual store. `--virtual-store-dir` now sets the global virtual store's location too [pnpm/tasks#47](https://github.com/pnpm/tasks/issues/47).

- `pnpm clean` no longer deletes the project when `virtualStoreDir` or `globalVirtualStoreDir` is set to the project directory. It also leaves a directory outside the project alone when the setting reaches it through a symlink. It now removes a global virtual store that `globalVirtualStoreDir` places inside the project, as it does for `virtualStoreDir`.

#### Optional dependencies

- `pnpm install` no longer fails when a dependency of an optional dependency is missing from the registry. Like npm, pnpm now leaves out the nearest optional dependency above it, together with its subtree [#16511](https://github.com/pnpm/pnpm/issues/16511).

- When an optional dependency fails to build, pnpm now removes its link from `node_modules`. A repeat `pnpm install` then reports "Already up to date" and no longer reruns the failing build [#16468](https://github.com/pnpm/pnpm/issues/16468).

- `pnpm install` now prints a warning with the error when an optional dependency cannot be fetched and is skipped. The skipped package is no longer counted in the `Packages: +N` summary. The `pnpm:skipped-optional-dependency` log reports the skip with the `fetch_failure` reason [#16514](https://github.com/pnpm/pnpm/issues/16514).

#### Resolving dependencies

- Fixed `pnpm install` changing an unchanged project's direct dependency to a sibling workspace's pinned version when its dependency tree contains a cycle [#16417](https://github.com/pnpm/pnpm/issues/16417).

- With `autoDedupe` enabled, downgrading a dependency in one workspace project now moves the other projects to that version when it satisfies their ranges. This also applies to a filtered `pnpm --filter <project> add` [#16432](https://github.com/pnpm/pnpm/issues/16432).

- `pnpm install` and `pnpm dedupe` now move an optional peer to the version already in the dependency graph when no other package provides its locked version anymore. After a bump such as `vue` 3.5.40 to 3.5.43, the lockfile kept a second copy of `@vue/server-renderer` for `@vue/test-utils` [#16443](https://github.com/pnpm/pnpm/issues/16443).

- `pnpm dedupe --check` no longer fails right after `pnpm install` when a project's optional peer is satisfied by a package another workspace project installs. `pnpm dedupe` now picks the same versions for that package's dependencies as `pnpm install` [#16447](https://github.com/pnpm/pnpm/issues/16447).

- `pnpm install` no longer re-resolves an up-to-date lockfile on every run when a patched package is a peer in a peer cycle [#16418](https://github.com/pnpm/pnpm/issues/16418).

- With `autoDedupe` enabled, `pnpm install --lockfile-only` no longer resolves the dependency graph again when nothing changed since an earlier `--lockfile-only` install deduplicated the lockfile. Such an install keeps the lockfile even if versions were published since it was written, or if only a setting such as `resolutionMode` changed. Run `pnpm dedupe` to apply such a change [#16458](https://github.com/pnpm/pnpm/issues/16458).

#### Speed and network

- A repeat `pnpm install` in a large workspace reports "Already up to date" faster [#16487](https://github.com/pnpm/pnpm/issues/16487).

- Sped up dependency resolution of workspaces with many peer dependencies.

- `pnpm install` sends fewer registry metadata requests when the lockfile already decides which version a range resolves to. This now also covers ranges that several locked versions satisfy when one of them outranks the others, and direct dependencies kept at their locked version. Packages that `minimumReleaseAgeExclude` lists without a version now reuse cached registry metadata the same way they do when `minimumReleaseAge` is not set [#16458](https://github.com/pnpm/pnpm/issues/16458).

- pnpm no longer downloads every packument again on each install from a registry whose metadata responses forbid caching, such as `Cache-Control: no-store`. pnpm revalidates the cached metadata with a conditional request, so a registry that supports conditional requests answers with a 304 when the package has not changed [#16528](https://github.com/pnpm/pnpm/issues/16528).

- Cached metadata for a package published within `minimumReleaseAge` is now revalidated with its ETag, so the npm registry can answer `304 Not Modified`. Before, the next install that checked the cache downloaded the whole document again [#16506](https://github.com/pnpm/pnpm/issues/16506).

- A fetch timeout while other downloads from the same host are still running now lowers concurrency for that host to one connection. Retries of that request, and later downloads from that host, use the lower concurrency. Other hosts keep the configured concurrency [#12791](https://github.com/pnpm/pnpm/issues/12791).

- Sped up `pnpm install --offline` when the version a range picks is not in the store. While it looks for a version the store holds, pnpm now reads only the versions the range admits [#16495](https://github.com/pnpm/pnpm/issues/16495).

- `pnpm install --offline` now reuses config dependency tarballs that are already present in the store [pnpm/tasks#46](https://github.com/pnpm/tasks/issues/46).

#### Running scripts

- `pnpm -s <script>` runs the script again, with `-s` meaning `--sequential` as it does for `pnpm run -s <script>`. pnpm rejected it with "unexpected argument '-s' found" [#16446](https://github.com/pnpm/pnpm/issues/16446).

- `pnpm run` and `pnpm exec` now warn and run the command when the install that `verifyDepsBeforeRun` starts fails. This lets scripts run in sandboxes where pnpm cannot install, such as containers with a read-only store or no network [#15173](https://github.com/pnpm/pnpm/issues/15173).

- A filtered `pnpm run` or `pnpm exec` now finds dependencies out of date when a workspace dependency of a selected project has no `node_modules` directory, as after a filtered install. With `verifyDepsBeforeRun: install`, pnpm installs that dependency before running the command [pnpm/tasks#45](https://github.com/pnpm/tasks/issues/45).

- Scripts run without a terminal no longer start a second `sh` each. One watchdog per pnpm command now ends every script's process group if pnpm is killed, so `pnpm -r run` across many projects starts half as many processes [#16489](https://github.com/pnpm/pnpm/issues/16489).

- `Terminate batch job (Y/N)?` no longer appears after pressing Ctrl+C in a script started with `pnpm` from PowerShell or cmd on Windows [#16502](https://github.com/pnpm/pnpm/issues/16502).

- `pnpm rebuild` and `pnpm approve-builds` refresh command launchers when a build changes a command's interpreter or replaces it with a native executable. Dependent packages' build scripts use the refreshed launchers.

- When `pnpm run <script>` or `pnpm <script>` finds nothing to run and `--filter` follows the script name, the error now suggests putting the filter option before the script name [#4655](https://github.com/pnpm/pnpm/issues/4655).

- Package-name filters now support `?` to match one character [#2817](https://github.com/pnpm/pnpm/issues/2817).

#### The pinned pnpm and `pnpm self-update`

- pnpm no longer downloads the project's pinned pnpm version again on every command when `nodeVersion` in `pnpm-workspace.yaml` names a different Node.js major than the `node` on `PATH`. Before, each of those commands took about a second longer and failed without network access [#16497](https://github.com/pnpm/pnpm/issues/16497).

- Several `pnpm` commands started at once in a project that pins `packageManager` no longer fail with `The process cannot access the file because it is being used by another process` on Windows while the pinned pnpm is being installed.

- pnpm can now switch to a `packageManager` version below 11 on x64 musl Linux, such as Alpine [#16467](https://github.com/pnpm/pnpm/issues/16467).

- A `devEngines.packageManager` range no longer makes pnpm replace the version recorded in `pnpm-lock.yaml` with the running pnpm while the recorded version still satisfies the range. When pnpm does record a version, it records the running pnpm only if it meets `minimumReleaseAge`. Otherwise it records the newest version in the range that meets it, or the running pnpm if none does [#16431](https://github.com/pnpm/pnpm/issues/16431).

- On Windows, `pnpm self-update` now replaces a `pnpm.exe` left in `PNPM_HOME` or in `PNPM_HOME\bin`. Windows ran that executable in place of the updated `pnpm.cmd` shim, so `pnpm --version` kept printing the old version after a successful update. If the executable was in `PNPM_HOME`, `self-update` now asks you to run `pnpm setup` [#9094](https://github.com/pnpm/pnpm/issues/9094).

- `pnpm self-update` now checks that a version installed as the JavaScript `pnpm` can start before making it the global `pnpm`. If Node.js is missing, the update fails and the current `pnpm` stays in place.

#### Other commands

- `pnpm deploy` now finds patches and local dependencies when the target directory sits under a symlink, such as `/tmp` on macOS. It failed with `ERR_PNPM_PATCH_NOT_FOUND` [#16470](https://github.com/pnpm/pnpm/issues/16470).

- `pnpm deploy --legacy` now resolves the deployed project's relative `file:`, `link:`, and path dependencies from the project's own directory [#16475](https://github.com/pnpm/pnpm/issues/16475).

- `pnpm update --global` now removes hard-linked executables from `PNPM_HOME` when migrating packages from the old global layout [#16420](https://github.com/pnpm/pnpm/issues/16420).

- `pnpm store prune` no longer fails on store index entries that pnpm 11 wrote for git-hosted packages without a `package.json`. Entries that still cannot be read are kept and counted in the prune summary.

- `pnpm store prune` now aborts when an error other than a missing directory occurs while scanning project directories in the mark phase.

- `pnpm config get` and `pnpm config list` now report a setting given on the command line with `--config.<name>=<value>`. Before, a value such as `--config.node-linker=hoisted` reached the install but was absent from the reported configuration [#16276](https://github.com/pnpm/pnpm/issues/16276).

- `pnpm -r pkg get` now reports every selected project when several share a package name. Projects with the same name are keyed by their directory relative to the workspace root. Before, only one of them appeared in the output.

- The install summary shows a `link:` dependency as `+ name <- path`, and the Node.js API's `hideLinkedPkgsDiff` reporter option leaves matching linked dependencies out of the summary.

- The `--force` help text of `pnpm install` and `pnpm add` now says that `--force` keeps skipping optional dependencies built for other platforms. It points to `forceIgnoresPlatform` and the `--os`, `--cpu`, and `--libc` options for installing them [#16435](https://github.com/pnpm/pnpm/issues/16435).

- The `homepage` field of the published `pnpm` package points to https://pnpm.io again.
