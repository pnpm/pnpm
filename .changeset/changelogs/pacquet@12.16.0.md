## 12.16.0

### Minor Changes

- Added a `--no-modify-path` option to `pnpm setup`. It installs pnpm without adding it to `PATH`, leaving the shell config file, the Windows registry, and the GitHub Actions environment files unchanged. `pnpm setup` then prints the lines to add yourself [get.pnpm.io#37](https://github.com/pnpm/get.pnpm.io/issues/37).

- Added a repeatable global `--env-file <path>` flag to load environment variables from dotenv-style files before running a command. Later files override earlier files, variables already in the environment are preserved, and loaded variables are visible to user-level `.npmrc` interpolation and lifecycle scripts [#7111](https://github.com/pnpm/pnpm/issues/7111).

- pnpm can now update a `pnpm-lock.yaml` that is a symlink. List the directories the lockfile may link into in the `symlinkedLockfileTargetDirs` setting, in the global config or with `PNPM_CONFIG_SYMLINKED_LOCKFILE_TARGET_DIRS`. pnpm writes the update to the file the symlink points to if that file is inside one of these directories. The symlink stays in place. pnpm ignores this setting in `pnpm-workspace.yaml` [#17222](https://github.com/pnpm/pnpm/issues/17222).

### Patch Changes

- `pnpm add` now accepts the lowercase short options `-p`, `-d`, `-o`, and `-e` as aliases of `--save-prod`, `--save-dev`, `--save-optional`, and `--save-exact`.

- `pnpm add <pkg>` without a version now installs the version under the dist-tag set by the `tag` setting. An `npm:` alias or a `jsr:` dependency without a version resolves that tag too. The setting is read from `pnpm-workspace.yaml`, the global config, `PNPM_CONFIG_TAG`, and `--config.tag`, and defaults to `latest`.

- `pnpm add <pkg>@<tag>` and `pnpm add <pkg>@<range>` now install the version the tag or range resolves to, even when the saved range allows a newer one [#17234](https://github.com/pnpm/pnpm/issues/17234).

- `pnpm audit --fix` and `pnpm audit --interactive` now fail with `ERR_PNPM_AUDIT_FIX_WITH_IGNORE` when combined with `--ignore` or `--ignore-unfixable`. Before, the fix ran and the ignore flags had no effect.

- `pnpm audit --ignore` and `pnpm audit --ignore-unfixable` now exit with a non-zero code when vulnerabilities at or above the audit level remain after ignoring. Before, these commands always exited with code 0.

- `strictSsl` and `localAddress` set in the global `config.yaml`, through `PNPM_CONFIG_STRICT_SSL` and `PNPM_CONFIG_LOCAL_ADDRESS`, or on the command line now apply to `pnpm self-update` and to the download of the pnpm version pinned in `packageManager`. These requests read them from `.npmrc` only before. `--config.localAddress` now reaches them in the camelCase spelling too [pnpm/tasks#201](https://github.com/pnpm/tasks/issues/201).

- `pnpm dedupe` now refreshes cached registry metadata while re-resolving locked versions [#17208](https://github.com/pnpm/pnpm/issues/17208).

- Fixed memory exposure and allocation handling vulnerabilities when processing zstd-compressed data [#17388](https://github.com/pnpm/pnpm/pull/17388).

- `pnpm ci --lockfile` and `pnpm ci --config.lockfile=true` no longer delete `pnpm-lock.yaml`. `pnpm ci` now always keeps the lockfile and runs a frozen install from it.

- Sped up cold installs and `pnpm fetch` on macOS. pnpm now links downloaded packages into the virtual store on a separate thread, so a slow link pass no longer pauses the downloads that are still in flight [#17271](https://github.com/pnpm/pnpm/issues/17271).

- When the `pnpm` package downloads its native binary on first run, as it does under Corepack, it now trusts the CA certificates from the `ca`, `ca[]`, and `cafile` settings of `.npmrc`, including the ones scoped to the registry. A registry behind a private CA failed with `fetch failed` before, unless `NODE_EXTRA_CA_CERTS` named the CA. A project `.npmrc` is used only while the download is checked against npm's signature. Node.js versions that cannot add CA certificates after startup still need `NODE_EXTRA_CA_CERTS`.

- When Corepack runs pnpm and the native binary has to be downloaded, the download now uses the proxy from pnpm's config. That proxy comes from `PNPM_CONFIG_HTTPS_PROXY` and the matching variables, the global `config.yaml`, and the user `.npmrc`. Previously, the download read the proxy only from `HTTPS_PROXY`, `HTTP_PROXY`, and `NO_PROXY` [pnpm/tasks#215](https://github.com/pnpm/tasks/issues/215).

- A custom fetcher in the pnpmfile can now call `fetchers.directory` to fetch a package from a local directory. The call rejects with `ERR_PNPM_TARBALL_INTEGRITY` if the lockfile pins an archive integrity for the package. Previously, calling `fetchers.directory` failed with a `TypeError`.

- `pnpm dedupe --check` no longer fails when the only difference is a `deprecated` notice that the registry added or removed after the lockfile was written [#17198](https://github.com/pnpm/pnpm/issues/17198).

- `pnpm deploy` now keeps a deployed symlink to a directory inside the project even if the directory is not among the deployed files, such as a link into `node_modules` [#17288](https://github.com/pnpm/pnpm/issues/17288).

- When `autoDedupe` is off, a workspace project's dependencies now resolve from its own ranges, like they do with `sharedWorkspaceLockfile: false`. Previously, another project's narrower range could hold them back. For example, `ajv: "*"` now resolves to the latest Ajv even if another project depends on `ajv@^6`. Packages that a project lists in `peerDependencies` still follow the versions other projects use. `pnpm dedupe` and installs with `autoDedupe: true` still pick one version that satisfies every range. Set `autoDedupe: true` if CI runs `pnpm dedupe --check` [#17235](https://github.com/pnpm/pnpm/issues/17235).

- `pnpm install` and the `verifyDepsBeforeRun` check now notice an edited patch file when `sharedWorkspaceLockfile` is `false`.

- `pnpm add -T` now reports that the short option is ambiguous and points to `--save-types` and `--tilde`.

- An install filtered to some workspace projects now fails if `node_modules` was installed with a different `nodeLinker`, `hoistPattern`, `publicHoistPattern`, store, or virtual store. Before, it recreated `node_modules` and left the projects it skipped without their dependencies. A `pnpm install` of the whole workspace still recreates `node_modules` [pnpm/tasks#217](https://github.com/pnpm/tasks/issues/217).

- A dependency symlink that pnpm cannot point at the wanted target now fails the install with an error that names the link. This happens when the filesystem still serves the old link after pnpm removed it, as a `node_modules` served by a virtual filesystem does. pnpm used to remove the link again without end and crashed with a segmentation fault once the thread ran out of stack.

- pnpm now builds a git-hosted dependency in every new location it links the dependency into, such as a new global virtual store slot or a fresh `node_modules`. When the side-effects cache holds the build output, pnpm restores it from there. Before, only the install that fetched the dependency built it. This also fixes dependencies that are already in the store [#17224](https://github.com/pnpm/pnpm/issues/17224).

- Warm installs no longer read the `package.json` of every git-hosted dependency. pnpm now stores the manifest of a git-hosted package in the store index when it fetches the package.

- `pnpm add -g` and the other `--global` commands now fail with `ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_DIR_WITH_GLOBAL` when `--virtual-store-dir` is passed. A custom `--hoist-pattern` fails with `ERR_PNPM_CONFIG_CONFLICT_HOIST_PATTERN_WITH_GLOBAL`. Passing `--link-workspace-packages`, `--shared-workspace-lockfile`, or `--config.lockfile-dir` with `--global` also fails, with the matching `ERR_PNPM_CONFIG_CONFLICT_*_WITH_GLOBAL` code.

- Global packages installed with `pnpm add -g` are now linked from the global virtual store, so global packages that share dependencies share one copy. Set `virtualStoreType: project` to give each global install its own virtual store.

- `pnpm help --all` and `pnpm help -a` now print the full list of commands. They used to fail with an "unrecognized subcommand" error.

- With `nodeLinker: hoisted`, an install that stops partway no longer leaves the next install trusting the packages it was replacing. Before, reverting the change that started the stopped install, such as a patch edit, kept the half-replaced package in `node_modules`.

- Under `nodeLinker: hoisted`, `pnpm install` now leaves a patched package that is already in place alone. The same holds for a package whose peers include a patched package, such as every package peering on a patched `react-native` [#17188](https://github.com/pnpm/pnpm/issues/17188).

- With `injectWorkspacePackages: true`, a `workspace:` dependency written as a relative path, such as `workspace:../lib-a`, in a workspace package now resolves from that package's directory [#17284](https://github.com/pnpm/pnpm/issues/17284).

- `pnpm -D install <pkg>` now saves the package to devDependencies. An `add` option such as `-D` or `-E` written before `install <pkg>` was rejected as an unexpected argument.

- `packageImportMethod: copy` now also applies to packages that have lifecycle scripts or patches. On a filesystem that supports block cloning, such as ReFS or a Dev Drive on Windows, these packages were cloned [#17195](https://github.com/pnpm/pnpm/issues/17195).

- `pnpm pack` and `pnpm publish` now keep `pnpm.config` in the published `package.json`. They also always include the file `pnpm.config` points to in the tarball [#17210](https://github.com/pnpm/pnpm/issues/17210).

- `pnpm deploy --legacy` now checks the workspace lockfile against `minimumReleaseAge` and the other resolution policies, as `pnpm install` does, and fails with `ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION` when a locked version is too new. It used to deploy an older version that is not in the lockfile without a warning [#17209](https://github.com/pnpm/pnpm/issues/17209).

- Directory selectors with `legacyDirFiltering: true` now include the project in the named directory along with its descendants. Excluding a directory also excludes that project [pnpm/tasks#196](https://github.com/pnpm/tasks/issues/196).

- An optional dependency written with a named-registry prefix, such as `work:^1.0.0`, now fails the install when it cannot be resolved but the lockfile already has a matching entry from that registry. Such a dependency was skipped, which removed its entries from the lockfile.

- `pnpm login` saves the token in `<pnpm config>/auth.ini` again. Since v12.1.0, an unscoped login had made its registry the default for every project that did not name a registry. A login now also removes the token an earlier login saved for that registry or scope under `_auth` in the global `config.yaml`. `pnpm login --scope` still routes the scope through the global `registries` setting.

- `pnpm install` resolves dependencies faster in large workspaces on Linux, macOS, and Windows on x64 and arm64.

- `pnpm install` now resolves a scope's packages again when the scope is routed to another registry in `registries`. Before, the lockfile kept the versions and integrity resolved from the old registry, so the install failed when the new registry did not serve a locked version or served different content. pnpm records in the workspace state which registry each scope routed to, so a fresh clone without that record installs the lockfile as recorded. Changing the default registry resolves nothing again, and `pnpm install --frozen-lockfile` is unchanged.

- `pnpm run`, `pnpm exec`, and `pnpm dlx` no longer dump a core file for `pnpm` itself when a command they started dies from a signal [#17204](https://github.com/pnpm/pnpm/issues/17204).

- Signing in pnpm and pnpr no longer uses arithmetic that may leak the private key through timing. This covers shared artifacts, OCI registry tokens, and OIDC login state [advisory](https://rustsec.org/advisories/RUSTSEC-2026-0387).

- pnpm now passes an interrupt on to the handler it replaced, with the arguments that handler expects. A handler that asked for the signal's details was called without them. Only a program that embeds pnpm and installs its own `SIGINT`, `SIGTERM` or `SIGHUP` handler was affected.

- `pnpm install` now takes its fast path in a workspace with `patchedDependencies` when the previous install ran with another major version of pnpm.

- `pnpm publish --batch` now sends `PUT /-/pnpr/v0/publish`. `pnpr` mounts the unified batch publish handler at both `/-/pnpr/v0/publish` and `/npm/-/pnpr/v0/publish`.

- Installs delegated to a pnpr server now resolve every prefix declared for a registry URL. A dependency or lockfile entry that used the second of two prefixes for the same registry, such as `zeta:` next to `work:` in `namedRegistries`, failed on the server [pnpm/tasks#204](https://github.com/pnpm/tasks/issues/204).

- Changes a `preResolution` hook makes to the lockfiles now take effect. The hook now also runs when pnpm installs from an up-to-date lockfile. An error thrown by the hook now fails the install.

- `pnpm install` now removes command shims from `node_modules/.bin` when a workspace dependency renames or removes an entry from its `bin` field [pnpm/tasks#211](https://github.com/pnpm/tasks/issues/211).

- With `nodeLinker: hoisted` and `autoInstallPeers: false`, a package that workspace projects use with different peer versions is now installed once per peer variant, so each project's copy resolves that project's peer [#17395](https://github.com/pnpm/pnpm/issues/17395).

- Reading the file-creation mask on Linux no longer sets it. pnpm had to replace the mask to learn what it was, and a file another thread created in the window between the two calls was created unmasked.

- `pnpm add`, `pnpm remove`, and `pnpm update` now fail if `node_modules` does not match the current settings. Run `pnpm install` to recreate it. Previously, these commands removed and reinstalled `node_modules` without asking. They report:

  - `ERR_PNPM_UNEXPECTED_STORE` if `node_modules` was linked from a different store.
  - `ERR_PNPM_UNEXPECTED_VIRTUAL_STORE` if it was linked from a different virtual store directory.
  - `ERR_PNPM_MODULES_BREAKING_CHANGE` if it was installed by an incompatible pnpm version, or if `node_modules/.modules.yaml` cannot be read.
  - `ERR_PNPM_NODE_LINKER_DIFF` if it was installed with a different `nodeLinker`.

  `pnpm install` now recreates `node_modules` after a `storeDir` or `virtualStoreDir` change. It used to report "Already up to date".

  `pnpm update` now also fails with `ERR_PNPM_HOIST_PATTERN_DIFF` and the other errors `pnpm add` reports for a changed hoist pattern or `virtualStoreDirMaxLength`.

- pnpm no longer warns that a `registries` entry routed through the `@` scope matches no configured registry. pnpm applied such an entry and its options such as `serverType` while reporting it as ignored [pnpm/tasks#184](https://github.com/pnpm/tasks/issues/184).

- A package that a plain range and a `<prefix>:` specifier both resolve from one registry is now recorded once in `pnpm-lock.yaml` and installed once. The lockfile key names the registry a package resolved from. When that registry declares a prefix, through `prefix` in `registries` or through `namedRegistries`, the package is keyed `<name>@<prefix>:<version>` however its dependency was written: with the prefix, as a plain range its scope routes to that registry, or through the default registry. Before, the plain range was keyed `<name>@<version>`, so one tarball was installed twice and `instanceof` checks and module singletons broke between the two copies. The same name and version served by two registries still get separate entries. When several prefixes declare one registry, its packages are keyed by the lexicographically smallest prefix.

  If a scope you route reaches a registry that also declares a prefix, your next `pnpm install` re-keys those entries, which shows up as a lockfile diff. Commit it. `pnpm install --frozen-lockfile` installs the lockfile as recorded.

- Fixed `pnpm install` failing with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a workspace package injected through a relative path, such as `workspace:../lib`, has `catalog:` dependencies. Their catalog entries now resolve the way they do for a package injected by name [#17280](https://github.com/pnpm/pnpm/issues/17280).

- A repeat `pnpm install` now links the packages from the new directory after `globalVirtualStoreDir` changes. Before, it reported "Already up to date".

- A repeat `pnpm install` now renames the virtual store directories after `virtualStoreDirMaxLength` changes. It now runs the builds it skipped before after `dangerouslyAllowAllBuilds` is turned on. It now verifies the lockfile after `trustLockfile` is turned off. Before, it reported "Already up to date" in each of these cases.

- `pnpm -r run <script>` now honors the `requiredScripts` setting from `pnpm-workspace.yaml`. If the script is listed there and a selected project does not define it, the run fails with `ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT` and names the projects that lack it.

- Registry revisions are now addressed at `<registry>/<name>@<version>+r<revision>` [pnpm/tasks#208](https://github.com/pnpm/tasks/issues/208).

- The `runtime` setting is now read. Setting `runtime: false` in `pnpm-workspace.yaml`, `pnpm_config_runtime=false`, or `--config.runtime=false` stops `pnpm install`, `pnpm add`, `pnpm dedupe`, and the other installing commands from installing the runtimes in `devEngines.runtime` and `engines.runtime`. `pnpm dedupe` now also accepts `--no-runtime`.

- pnpm no longer documents the `scriptsPrependNodePath` setting, which has no effect. A `pnpm-workspace.yaml` that sets it still loads.

- Sped up installs that add new projects to a workspace with a peer-heavy lockfile. pnpm now reads the lockfile's locked peer versions once for all new projects [#17311](https://github.com/pnpm/pnpm/issues/17311).

- Concurrent workspace installs no longer lose store-index rows when initializing the shared SQLite index [#17388](https://github.com/pnpm/pnpm/pull/17388).

- Agent skill links now keep the same target across installs when a package version is installed with several sets of peer dependencies. An install no longer fails with `ERR_PNPM_AGENT_SKILL_PATH_OCCUPIED` on a skill link that pnpm created for another of those copies [pnpm/pnpm#17319](https://github.com/pnpm/pnpm/issues/17319).

- Fail immediately without retrying when a downloaded package archive contains a rejected entry path [#17205](https://github.com/pnpm/pnpm/issues/17205).

- `pnpm store prune` now keeps the packages that a project still uses when pnpm copied or cloned them into `node_modules`. These include patched packages, built packages, and every package installed with `packageImportMethod: copy`. Before, the prune removed them, and the next install downloaded them again [#7192](https://github.com/pnpm/pnpm/issues/7192).

- `pnpm store prune` now removes the package metadata cache, the `metadata*` directories in the cache directory. This clears the outdated integrity checksums that cause `ERR_PNPM_TARBALL_INTEGRITY` when a registry republishes a version.

- `pnpm store prune` now keeps the packages of a project when the store path reaches its directory through a symlink. Previously, prune dropped such a project from its list and removed the packages only that project used from the global virtual store.

- Output that fills the terminal, such as `pnpm run --stream` script output, is now written once. Before, every new line also rewrote all the lines already on screen [#17387](https://github.com/pnpm/pnpm/issues/17387).

- pnpm now reads the `strictSsl` and `localAddress` settings from `pnpm-workspace.yaml`, the global `config.yaml`, `PNPM_CONFIG_*` environment variables, and `--config.*` options. Before, pnpm read them only from `.npmrc` as `strict-ssl` and `local-address` and ignored the other sources.

- `pnpm update --depth <n>` with a large `n`, such as the `--depth=9999` that `update-browserslist-db` runs, now finishes as fast as `--depth Infinity`. It used memory and stack in proportion to `n` on lockfiles with dependency cycles, and could run out of memory or crash [#17329](https://github.com/pnpm/pnpm/issues/17329).

- `pnpm run` and `pnpm exec` no longer report the dependencies as out of date after a lockfile change when `dedupePeers` is `true` [#17322](https://github.com/pnpm/pnpm/issues/17322).

- pnpm now warns when a pnpmfile defines the `importPackage` hook. pnpm does not run this hook. Use the `packageImportMethod` setting to choose how packages are written to `node_modules`.

- `pnpm` now warns when a project's `pnpm-workspace.yaml` sets `ci`, `globalShims`, or `tools`, and names where to set them instead. These settings were already ignored there without a warning. `pnpm config set` also refuses to write them to a project's `pnpm-workspace.yaml`.

- On macOS, killing pnpm while it runs scripts in several projects at once now ends every one of those scripts. Before, a script could keep running after pnpm died.

- `pnpm install` now relinks the bins of a workspace package when its `bin` field changes, including with `--frozen-lockfile` and an unchanged lockfile. A filtered install relinks them in the selected projects, and the next install that covers the remaining projects relinks them there [pnpm/pnpm#17324](https://github.com/pnpm/pnpm/issues/17324).

- Updated dependencies:
  - get-pnpm@0.0.10
