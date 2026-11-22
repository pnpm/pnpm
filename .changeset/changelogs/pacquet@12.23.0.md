## 12.23.0

### Minor Changes

- Added the `pnpm edit` command to open an installed package directory in an editor.

- pnpm now provides a prebuilt binary for Linux on LoongArch (loong64) with glibc.

- Added `pnpm catalog migrate`, which moves dependency specifiers into the default catalog and replaces them in the workspace projects' `package.json` files with `catalog:`. A dependency moves only when every project declares it with the same specifier. The command lists the dependencies it leaves unchanged [#7072](https://github.com/pnpm/pnpm/issues/7072).

- Added the `versioning.includePrivatePackages` setting. Set it to `false` to leave private packages out of `pnpm change` [#13813](https://github.com/pnpm/pnpm/issues/13813).

- Add `--long` to `pnpm update --interactive` to fetch and display package homepage or repository URLs. The URL column is hidden by default.

- Deprecation warnings moved into the install summary and only name the packages the install added. A deprecated dependency added by the install is marked `deprecated` on its summary line, with the version that is not deprecated when there is one. Deprecated subdependencies the install added are listed right after the summary. Deprecated packages that were already installed are reported as one count, such as `3 deprecated packages are still installed`.

- `pnpm doctor` now checks the environment that lifecycle scripts run in. It lists the `node` executables on `PATH` and warns about broken ones. It reports the shell that runs scripts. A new check runs a package's executable from a script, both in a temporary project and in the current project. If that script fails, the report includes the error and, outside Windows, a trace of the executable's shim that shows how it looked up and started `node` [#16308](https://github.com/pnpm/pnpm/issues/16308).

- `pnpm doctor` now warns in its "Install method" check when the `pnpm` found on PATH is a copy pnpm did not install. The warning names that copy. If pnpm recognizes its installer, it also gives the command that removes it.

- Use `pnpm audit --fix --no-add-release-age-exclusions` to apply audit fixes without adding entries to `minimumReleaseAgeExclude`.

- When pnpm updates a `package.json5`, it now changes only the values that changed. Comments, quotes, number formats, and layout elsewhere in the file stay as written. New keys and strings use the quote style of the file.

- Added a `--hard` option to `pnpm link`. With `--hard`, pnpm saves the linked package with the `file:` protocol, so its files are hard linked into `node_modules` and its dependencies and peer dependencies resolve from your project. `pnpm unlink` removes these links too [#9895](https://github.com/pnpm/pnpm/issues/9895).

- `pnpm install` now shows lockfile verification progress while the supply-chain check runs. The final pass or failure line reports how many entries were checked.

- Added the `--minimum-release-age` and `--minimum-release-age-exclude` options to `pnpm install`, `pnpm add`, `pnpm update`, and `pnpm dlx`. They set `minimumReleaseAge` and `minimumReleaseAgeExclude` for a single run without editing a config file [#11224](https://github.com/pnpm/pnpm/issues/11224).

- `nodeLinker` accepts object settings for isolated and hoisted installs. Linker-specific hoisting options can be configured alongside the selected linker.

- `pnpm pack --json` now reports the `package.json` that goes into the tarball, with `publishConfig` and the other publish-time changes applied. Run `pnpm pack --dry-run --json` to preview it without writing a tarball. A dry run still runs `prepack` and `prepare`, unless `--ignore-scripts` is passed.

- `pnpm pack` and `pnpm publish` now add a workspace-root `COPYING` file to a workspace package that has no license file. A `COPYING` file at the package root now counts as the package's license, so the workspace-root `LICENSE` is no longer added next to it.

- Set `PNPM_CONFIG_HOME` to choose the directory containing `config.yaml` and `auth.ini`. Global configuration commands read and write files in this directory. A non-empty value takes precedence over `XDG_CONFIG_HOME` and the platform defaults.

- `pnpm pack` and `pnpm publish` now fail if a package that is not private lists a private workspace package in `dependencies`. A private package is never published, so the published package could not be installed. `devDependencies`, `optionalDependencies`, `peerDependencies`, and bundled dependencies are still allowed [pnpm/pnpm#2321](https://github.com/pnpm/pnpm/issues/2321).

- `pnpm publish` now supports `--new-version <version>`, which sets the version in `package.json` before publishing. With `--recursive`, every selected workspace package is set to the new version [#2168](https://github.com/pnpm/pnpm/issues/2168).

- Added the `referenceTime` setting. `minimumReleaseAge` and `trustPolicyIgnoreAfter` count back from this RFC 3339 time, both when pnpm resolves versions and when it verifies a lockfile. Set it in `pnpm-workspace.yaml` or with `PNPM_CONFIG_REFERENCE_TIME`.

- `pnpm remove` now accepts glob patterns and `!` exclusions in dependency selectors, such as `pnpm remove "eslint-*" "!eslint-plugin-import"` [#13546](https://github.com/pnpm/pnpm/issues/13546).

- Removed platform variants. A package version has one archive again.

  pnpm no longer reads `publishConfig.variants` or the `variant` field of `package.json`. `pnpm pack --variant` and the `--upload-variant`, `--variant-integrity` and `--no-variants` options of `pnpm publish` are gone. pnpr no longer accepts variant archives with a version or serves them at `<name>@<version>/variants/<tag>`.

  To ship builds for several platforms, publish each as its own package and list them in `optionalDependencies`. A frozen install of a lockfile that records the variants of a registry package fails with `ERR_PNPM_BROKEN_LOCKFILE`. Run `pnpm install` to resolve the lockfile again.

- A failed `devEngines.runtime` or `engines.runtime` check now reports the reason in its error code. A runtime that is not installed fails with `ERR_PNPM_RUNTIME_NOT_FOUND`. A runtime entry without a version range fails with `ERR_PNPM_MISSING_RUNTIME_VERSION_RANGE`, and one with an invalid range fails with `ERR_PNPM_INVALID_RUNTIME_VERSION_RANGE`. An installed version that does not satisfy the range still fails with `ERR_PNPM_BAD_RUNTIME_VERSION`.

- `--allow-build` now accepts a comma-separated list of packages, so `pnpm add foo bar --allow-build=foo,bar` is the same as passing `--allow-build=foo --allow-build=bar` [#9560](https://github.com/pnpm/pnpm/pull/9560).

- With `trustPolicy: no-downgrade`, `pnpm update --interactive` shows a `Trust` column with the trust evidence of each offered registry dependency [#8889](https://github.com/pnpm/pnpm/issues/8889). The list also offers only versions the trust policy accepts, so it matches what the update installs.

- `pnpm update --latest <pkg>` now moves the override that pins the named dependency [#8701](https://github.com/pnpm/pnpm/issues/8701). `pnpm update --interactive --latest` does the same for every selected dependency. An override keeps its own range shape: a pin on an exact version lands on the new exact version, and `^` stays `^`. When pnpm cannot update the governing override automatically, the update prints a warning naming it.

- `pnpm update` now accepts `--tag <tag>`. The flag updates the matched dependencies to the version behind the given dist-tag and rewrites the ranges in `package.json`, the way `--latest` updates to the `latest` tag. It also works with `--interactive` and with global updates [pnpm/pnpm#3534](https://github.com/pnpm/pnpm/issues/3534).

- `pnpm view <pkg>` now prints the registry it read the metadata from at the end of the summary [#11394](https://github.com/pnpm/pnpm/pull/11394).

- `pnpm self-update` and `pnpm setup` now warn when another `pnpm` on PATH comes before the global bin directory, because the shell would keep running that copy. The warning names that copy. If pnpm recognizes how it was installed (npm, Homebrew, Corepack, Volta, or Scoop), the warning also gives the command that removes it.

- `pnpm install` now warns when an entry in `overrides` matches no dependency, in a single line such as `2 overrides matched no dependency: foo, parent>child`. Only the overrides declared in the workspace are checked. Overrides added by a config dependency or an `updateConfig` hook are not. Set `warnUnusedOverrides: false` to turn the warning off [#10315](https://github.com/pnpm/pnpm/issues/10315).

- In a workspace, the install summary now lists the direct dependency changes of every project, not only of the project pnpm runs in. Each change is listed once, followed by the projects that made it, unless only the current project did. A project's first install lists only deprecated or outdated dependencies and the ones the command added [#6481](https://github.com/pnpm/pnpm/issues/6481).

### Patch Changes

- The peer dependency warning of `pnpm install` no longer reports a `catalog:` peer of an injected workspace package as unmet when the installed version matches the catalog entry [#10279](https://github.com/pnpm/pnpm/issues/10279).

- `pnpm peers check` now resolves the `catalog:` peer ranges of injected workspace packages through the workspace catalogs. With `autoInstallPeers: false`, such a peer was reported as unmet even when the installed version matched the catalog entry [#10279](https://github.com/pnpm/pnpm/issues/10279).

- `pnpm install` now adds a new workspace package that has no dependencies to the `importers` of `pnpm-lock.yaml` [#18189](https://github.com/pnpm/pnpm/issues/18189).

- With `enableGlobalVirtualStore` on, `require.extensions` hooks run again for `require()` calls from a module compiled from a string, such as the module Next.js uses to load `next.config.ts`. A required `.ts` file skipped the registered TypeScript hook. A required `.json` file was parsed as JavaScript [#18261](https://github.com/pnpm/pnpm/issues/18261).

- Fixed `pnpm install` failing with `ERR_PNPM_NEXT_LOCKFILE_NOT_REPRESENTABLE` under `lockfile.format: v13` when a peer dependency is provided by an aliased package and another alias installs the package that has the peer's name [#18198](https://github.com/pnpm/pnpm/issues/18198).

- `pnpm add --allow-build` now records its decision under `permissions` when `pnpm-workspace.yaml` has a `permissions` setting. Before, a `--allow-build=!<pkg>` denial went to `allowBuilds` and was overridden by an existing `permissions.<pkg>.build: true`.

- `peerDependencyRules.allowedVersions` now accepts a parent version range that contains `>`, such as `"foo@>=1.0.0>bar": "1"`.

- `pnpm approve-builds --all` and `pnpm approve --all` no longer approve a package whose build is denied with `false` in `pnpm-workspace.yaml`.

- `pnpm audit` now checks patched versions correctly when registry metadata includes an `unpublished` entry in its `time` field [#18209](https://github.com/pnpm/pnpm/issues/18209).

- `pnpm audit` now checks tarball, git, and named-registry dependencies against security advisories. `pnpm audit signatures` now verifies packages installed from named registries and skips non-registry dependencies.

- pnpm recognizes uppercase file extensions when choosing the interpreter for commands without a shebang. It skips hidden files and directories when linking commands from `directories.bin`.

- Fixed `pnpm run` waiting forever after an interrupt when a script left behind a process that ignores it. pnpm now waits thirty seconds for the script's processes to stop and then exits, leaving any that are still running.

- `pnpm deploy` now resolves relative `file:` overrides from the source workspace [#18191](https://github.com/pnpm/pnpm/issues/18191).

- Install reports packages awaiting build and skill approval in one notice.

- `pnpm list --lockfile-only --find-by` now reads manifests from the configured store when `node_modules` is absent.

- `pnpm licenses list` now reads cached package metadata and license files when `node_modules` is absent.

- A `link:` or relative-path dependency of a local directory dependency now resolves from that directory. In any other package, such as a local tarball, it resolves from the lockfile directory. Both used to resolve from the importing project's directory [pnpm/tasks#319](https://github.com/pnpm/tasks/issues/319).

  Two workspace projects that declare the same injected relative-path dependency, such as `../child`, no longer share the directory the first one resolved.

- When pnpm cancels a script, it now waits up to ten seconds for the system's process list. A process that a cancelled script started is no longer left running on a machine that answers more slowly than half a second.

- `pnpm install` now locks Cargo path dependencies outside the workspace, as Cargo does. It failed with `workspace dependency <name> is not a workspace member`. `pnpm install --frozen-lockfile` now also fails when `Cargo.lock` misses a path dependency or locks a git dependency at another branch, tag, or revision.

- `pnpm outdated` and `pnpm update` now include Cargo dependencies declared under `[target.<cfg>.dependencies]`. `pnpm update --prod` no longer fails on a crate that is also a dev-dependency. `pnpm add crate:` fails in a virtual Cargo workspace root, as `cargo add` does. It added a `[dependencies]` table that Cargo rejects there.

- Frozen installs now detect changes to catalog entries used only by peer dependencies of local directory packages.

- `pnpm install` now fails with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a `file:` dependency that is not a workspace project uses a `catalog:` specifier. Injected workspace projects still resolve their `catalog:` dependencies.

- `pnpm approve-builds` now requires the exact selector shown by `pnpm ignored-builds` for a dependency a bare package name cannot identify (a `file:`, git, or tarball artifact). A bare package name only approves a plain registry version [#18237](https://github.com/pnpm/pnpm/issues/18237).

- Validate `--config.<camelCase>` CLI setting values, route `--config.storeDir` and `--config.stateDir` through the global `--store-dir` and `--state-dir` flags, and accept empty `--state-dir=` values to reset to the default state directory, matching `--store-dir=`.

- `pnpm cache list`, `view`, and `delete` search across registries by default. Pass `--registry` to restrict them to one registry.

  `pnpm find-hash` reports an unknown hash when the store is empty. Inspection commands with `--json` write errors to stdout. `pnpm doctor` checks the resolved global bin directory on PATH.

  `pnpm list -r --no-sort --reverse` reverses the selected project order. Recursive runs honor `--sequential` with `--parallel`. Changed-file ignore patterns support negation.

  `pnpm clean` forwards positional arguments to a project's clean script. Shell completion uses the cursor position when editing an existing command.

- Tab completion suggests project scripts when you type `pnpm` followed by a script name [#18239](https://github.com/pnpm/pnpm/issues/18239).

- The help for `pnpm config get` and `pnpm get` now says that the key is a property path, such as `catalog.react`.

- `pnpm install` now converts an existing lockfile when you change `lockfile.format`, even when all dependencies are already installed.

- `pnpm dedupe --check` no longer requests the registry metadata of locked versions that the cache already holds. `pnpm dedupe` skips those requests too when `lockfile.format` is `v13`, which records no `deprecated` notices. Both were slow against a registry that sends no `ETag` or `Last-Modified`, such as Azure Artifacts, where each request downloaded the full package metadata [#17758](https://github.com/pnpm/pnpm/issues/17758).

- During dependency filtering in commands such as `pnpm install --prod --frozen-lockfile`, pnpm now recovers hashed peers in v9 lockfiles only when a binding can affect which dependencies are kept [#18276](https://github.com/pnpm/pnpm/issues/18276).

- Fixed `pnpm deploy` for projects that depend on an external directory through `link:`.

- When `sharedWorkspaceLockfile` is `false`, `pnpm install` now runs the workspace root's `pnpm:devPreinstall` script once, before any project is installed, including when a filter excludes the root. Workspace members no longer run theirs [pnpm/pnpm#4503](https://github.com/pnpm/pnpm/issues/4503).

- Sped up `pnpm install` on macOS for packages installed with several sets of peer dependencies. pnpm now clones every peer variant of a package from one cached copy of its files.

- Resolving a remote tarball uses the download response directly, saving a request. Requests to `pnprServer` send credentials only over HTTPS or loopback HTTP.

- `pnpm dlx`, `pnx`, and `pnpm create` now handle local isolation, script skipping, and non-registry package specifiers consistently:

  - `pnpm` ignores local `.pnpmfile.cjs` hooks and project `.npmrc` proxy or TLS settings in the working directory.
  - Running with `--ignore-scripts` or `ignoreScripts: true` uses a separate `dlx` cache key, keeping cached installs that skipped build scripts separate from normal runs.
  - Relative local directory and tarball specifiers such as `./my-cli` and `./my-cli.tgz` resolve against the caller's working directory.
  - Explicitly requested tarball URL, `file:`, and git URL packages run their build scripts by default unless `--ignore-scripts` or a negated `--allow-build` selector disables them.

- `pnpm dlx`, `pnx`, and `pnpm create` now ignore untrusted execution, layout, and resolution settings in a local `pnpm-workspace.yaml`, including `pnpmfile`, `globalPnpmfile`, `configDependencies`, `scriptShell`, `cacheDir`, `storeDir`, and `ignoreScripts`. Settings from the user's global `config.yaml`, such as `blockExoticSubdeps`, `cacheDir`, and `storeDir`, remain active.

- `pnpm install` and `pnpm install --frozen-lockfile` now enforce `blockExoticSubdeps` against entries in `pnpm-lock.yaml`. Before, pnpm accepted a lockfile with git or tarball URL subdependencies and installed those packages even with `blockExoticSubdeps: true`.

- Frozen installs now restore links declared through catalogs when `excludeLinksFromLockfile` is enabled.

- `pnpm install` with `lockfile.format: v13` now writes `pnpm-lock.yaml` in the v13 format when it updates an existing v9 lockfile without resolving, for example after a dependency is removed. Such an install used to keep the v9 format.

- Fixed `pnpm fetch` failing when a Docker build copies the lockfile before the workspace configuration or pnpmfile.

- `pnpm audit --fix=true` now applies the default override fix method, like a bare `pnpm audit --fix` [#13261](https://github.com/pnpm/pnpm/issues/13261).

- `pnpm pipeline` invalidates cached results when values supplied by `--env-file`, `extraEnv`, or `NODE_OPTIONS` change, or when the script shell settings change. Declared outputs of sibling tasks no longer invalidate the default inputs, and output globs support `!` exclusions.

  `pnpm pipeline` respects `--filter` and `--filter-prod`, includes untracked files when selecting affected projects, and runs the prerequisites of tasks matched by regex selectors.

  With `shellEmulator: true`, parameter expansions such as `${MODE:-dev}` now see assignments made earlier in the script.

- `pnpm --version` now succeeds when the project pins another package manager.

- The pnpm npm package provides bundled node-gyp to lifecycle scripts even when pnpm's own install scripts were skipped [#18227](https://github.com/pnpm/pnpm/issues/18227).

- `pnpm install --frozen-lockfile` no longer waits for a Node.js version check it does not use before it exits. With Homebrew's Node.js, a frozen install with nothing to do can be about 3x faster.

- When the `pnpm` package downloads its native binary on first run, it now reads the registry and credentials from the global `auth.ini`, where `pnpm login` writes them, and from the `_auth` setting of the global `config.yaml`.

- When the `pnpm` package downloads its native binary on first run, a `${NPM_TOKEN?}` placeholder in `.npmrc` now expands to the variable. It expanded to an empty value, so the download was sent without credentials.

- When the `pnpm` package downloads its native binary on first run, credentials configured for a registry path, such as `//registry.example.com/org-a/npm/:_authToken`, are now sent only to that path. They were sent to any path on the same host.

- Fixed `pnpm update` and `pnpm outdated` with `--include-github-actions` corrupting workflow YAML or skipping actions referenced through YAML anchors, aliases, or merge keys.

- `pnpm add -g` with several space-separated packages now lists every installed package in its summary. Only the packages of the first one were listed.

- `pnpm list -g` with `--depth` greater than `0` now always reads the global install directory's `node_modules` and lockfile. Previously it could read the current project's lockfile or a custom `modulesDir`.

- `pnpm add -g`, `pnpm update -g`, and `pnpm outdated -g` now use the `_auth` and `npmrcAuthFile` settings under `globalPackagesConfig` in the global `config.yaml`.

- Commands that act on global packages, such as `pnpm add -g`, now honor `dangerouslyAllowAllBuilds` set at the top level of the global `config.yaml`. pnpm ignored it for global packages unless it was set under `globalPackagesConfig`.

- `pnpm add` run from a workspace package directory that has no `package.json` yet now suggests creating one with `pnpm init` [#11345](https://github.com/pnpm/pnpm/issues/11345).

- On Windows, concurrent installs that link the same dependency no longer leave an `.ignored_<name>` entry next to the link in the global virtual store.

- With `nodeLinker: hoisted`, pnpm now finds the agent skills of a dependency whose directory is shared with another peer variant of the same package version.

- Fixed `nodeLinker: hoisted` installs in a workspace where the directory of one project is inside the directory of another. A dependency of the inner project could resolve to the version in the outer project's `node_modules`. pnpm now keeps the inner project's dependency in its own `node_modules` when the outer project holds another version.

- `pnpm install` with `nodeLinker: hoisted` no longer hangs or runs out of memory in large workspaces whose packages are installed with many different sets of peer dependencies.

- Fixed pnpm dropping Ctrl+C just before launching its native binary through Corepack or Node.js on Unix [#18222](https://github.com/pnpm/pnpm/issues/18222).

- `pnpm import` respects the symlink policy for the destination lockfile and skips Yarn patches outside the workspace.

- `pnpm import` now writes each project's lockfile when `sharedWorkspaceLockfile` is disabled and `lockfileDir` is unset. It respects `disallowWorkspaceCycles`. It normalizes workspace-relative Yarn patch paths and accepts Yarn Classic entries identified by a URL or file path.

- `pnpm cat-file` can read executable files from the store. `pnpm cache list`, `view`, and `delete` recognize mixed-case package names.

  `pnpm clean` removes custom virtual stores with hidden directory names. In Plug'n'Play projects it also removes the generated loader on a full clean.

- The install summary shows "(X is available)" again after a direct dependency whose `latest` version on the registry is newer than the one installed. The note appears when the install resolved that dependency against registry metadata.

- Global commands (`--global`, `-g`) no longer load `.pnpmfile.cjs`, `pnpm-workspace.yaml`, or `.npmrc` from the current working directory. `pnpm why -g` now inspects globally installed packages.

- Fixed `pnpm install --lockfile-only` fetching a locked transitive Git dependency when an unrelated dependency changes [#18217](https://github.com/pnpm/pnpm/issues/18217).

- `pnpm patch-remove` now removes the corresponding edit directory and state entry from `node_modules/.pnpm_patches`.

- Sped up linking bins in large workspaces. pnpm now reuses a workspace package's `package.json` across the projects that depend on it when linking their bins.

- Fixed `pnpm link` for target manifests without a name. The command uses the directory name and normalizes relative paths written to the manifest.

- Fixed `pnpm link` resolving a quoted `~/` path from the current directory.

- Fixed conversion to the v13 lockfile format when a hashed peer suffix includes a workspace link and the package installs its own copy of another declared peer [#18288](https://github.com/pnpm/pnpm/issues/18288).

- `pnpm list --json` now reports unsaved dependencies from each workspace project's own modules directory [#18248](https://github.com/pnpm/pnpm/issues/18248).

- `pnpm run` and `pnpm exec` now find the Plug'n'Play loader and experimental package map when `lockfileDir` is configured. This also applies to recursive commands.

- The `pnpm` binary no longer includes the `chrono` crate, which RustSec reports as unmaintained in RUSTSEC-2026-0543. Timestamps are read and written as before.

- A `<pkg>@<version>: false` entry in `allowBuilds` or `permissions` now also blocks `<pkg>@<version>` from a named registry, including when `dangerouslyAllowAllBuilds` is set.

- `pnpm pack` and `pnpm publish` now validate and normalize the manifest returned by `beforePacking`.

- A `tasks` entry with no value in `pnpm-workspace.yaml`, such as `build:`, now fails with a hint. The hint suggests writing `{}` for a task with no settings, or removing the entry to keep the default `dependsOn: ['^<task>']`.

- Fixed writing v13 lockfiles when a package has an ordinary npm alias of one of its direct or transitive peers [#18274](https://github.com/pnpm/pnpm/issues/18274).

- `otp` is no longer read from any config file. Pass `--otp` on the command line, or set `PNPM_CONFIG_OTP`. A `pnpm-workspace.yaml` or a global `config.yaml` that carries `otp` is ignored, with a warning naming those two channels. `pnpm config set otp` fails with `ERR_PNPM_CONFIG_SET_NOT_A_FILE_SETTING` instead of writing an entry no loader reads back.

  This closes a way to leak a secret. A repository could write `otp: ${NPM_TOKEN}` beside its own `registry:`, and `pnpm publish` sent that environment variable to the registry the same file named, in the `npm-otp` header of its first request [#13542](https://github.com/pnpm/pnpm/issues/13542).

- `pnpm outdated` now reports newer prereleases of an installed prerelease. It checks the dist-tag that points to the newest version of the installed prerelease channel, such as `next` for `3.0.0-rc.0`, alongside `latest` [#7339](https://github.com/pnpm/pnpm/issues/7339).

- When an overrides entry requests a version that does not exist on the registry, `pnpm install` now names that override entry in the error. If the entry's selector has a version range, such as `foo@^1`, the error also shows the latest published version in that range.

- Fixed packing relative workspace dependencies with a `package.yaml` or `package.json5` manifest. Packages with these manifests now include their parked changelog when packed.

- Packed workspace dependencies now use the dependency's `publishConfig.name`.

- `pnpm publish` now reads `access` and `tag` from `pnpm-workspace.yaml`, the global `config.yaml`, and `PNPM_CONFIG_ACCESS` / `PNPM_CONFIG_TAG`. The `--access` and `--tag` flags still win when set. A configured `access` outranks `publishConfig.access`, as it does in pnpm. An empty `publishBranch` now allows `master` and `main` [#13542](https://github.com/pnpm/pnpm/issues/13542).

- Fixed several bugs in applying and committing patches:

  - A patch that only changes a file's mode no longer corrupts a binary file.
  - A patch with CRLF line endings now applies when a blank context line has lost its leading space.
  - A unified diff without `diff --git` lines now applies.
  - A patch that creates a symlink now fails with `ERR_PNPM_INVALID_PATCH`. Previously pnpm wrote the symlink as a regular file.
  - `pnpm patch-commit --patches-dir ../outside` now fails with `ERR_PNPM_PATCHES_DIR_OUTSIDE_PROJECT`. Previously pnpm wrote the patch to `outside` inside the project.

- `pnpm patch-commit` keeps the peers a patched package is bound to in the lockfile. It no longer fails with `ERR_PNPM_NEXT_LOCKFILE_NOT_REPRESENTABLE` when the package has an optional peer and `lockfile.format` is `v13`.

- `pnpm patch-commit` now keeps a file without a final newline unterminated when that file comes last in the patch. Previously the patch dropped the final `\ No newline at end of file` marker, so the patched file gained a trailing newline.

- A name-only `patchedDependencies` entry now records its patch hash on a `file:` directory dependency or an injected workspace package. `pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_INCONSISTENT_PATCH_HASH` on the lockfile that `pnpm install` wrote.

- `pnpm patch` and `pnpm patch-commit` now find a package installed from a `namedRegistries` alias by its version. For example, `pnpm patch foo@1.0.0` patches a dependency declared as `work:1.0.0`.

- With `sharedWorkspaceLockfile: false`, `pnpm install` now reports missing peers of linked workspace packages, as `pnpm peers check` does.

- The peer dependency check now reads the version of an injected workspace package from its `package.json`. `pnpm install` with `strictPeerDependencies: true` and `pnpm peers check` no longer report such a peer as unmet with a `file:` version.

- `pnpm approve-builds`, `pnpm approve`, and `pnpm add --allow-build` now refuse to edit a package's `permissions` entry written as an inline mapping that spans several lines. Before, they added a second key for the package to `pnpm-workspace.yaml`.

- Matching a glob that repeats a wildcard, such as `+(*)z` in the workspace `packages` patterns, no longer hangs on long paths. The bundled picomatch is patched for a [security advisory](https://github.com/advisories/GHSA-fc5q-39xc-vjpf).

- `pnpm pipeline` no longer deletes files from a build script's output directory when it restores a shared Cargo build cache. Before, it could delete a `fingerprint` or `.fingerprint` directory there, such as one in a nested Cargo build.

- `pnpm install --frozen-lockfile` now accepts a plain-range dependency of a local directory dependency that `linkWorkspacePackages: deep` linked to a workspace project, and rejects the lockfile when its `injected` setting no longer matches the recorded reference.

- Plug'n'Play now combines custom export conditions from `NODE_OPTIONS` and command-line flags. It also respects Node.js's enabled `node-addons` and `module-sync` conditions.

- Plug'n'Play now respects package exports and `require.resolve()` lookup paths. `pnpm install` recreates a deleted `.pnp.cjs` loader.

- `pnpm install` with `pnprServer` now rejects registry-keyed lockfile entries whose resolution uses a non-registry source with `ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`. The `pnpr` lockfile verification and resolution endpoints enforce the same check before accepting or caching a lockfile.

- `pnpm install` with `pnprServer` now applies `injectWorkspacePackages`, `dedupeInjectedDeps`, `ignoredOptionalDependencies`, and `dependenciesMeta`. Projects that use them no longer fail with `ERR_PNPM_OUTDATED_LOCKFILE` or `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH`.

  `pnpm install --update-checksums` with `pnprServer` now refreshes the integrity checksums in the lockfile.

  With `pnprServer` set and `blockExoticSubdeps: false`, pnpm resolves locally.

- `pnpm install` with `pnprServer` configured now falls back to local resolution when the project declares `file:`, `link:`, or local path dependencies. `pnpr` also rejects local filesystem dependency specifiers on `/-/pnpr/v0/resolve`.

- Installs through a pnpr server now enforce `minimumReleaseAgeStrict` and add fallback versions to `minimumReleaseAgeExclude` when strict mode is off.

- The workspace project graph now follows `overrides`. An override that points a dependency at a workspace package, such as `foo: workspace:*`, makes that package a dependency of every project whose declaration the override matches, even with `linkWorkspacePackages` off. `pnpm install` now runs that package's lifecycle scripts before the scripts of the projects depending on it. `pnpm -r run` orders the projects the same way. `--filter` selectors that follow dependencies now see the edge.

- Fixed `pnpm prune` leaving extraneous package links in hoisted installations.

- Fixed `pruneDirectDependencies` skipping cleanup when the lockfile was already up to date.

- Fixed `pnpm update` failing with a TOML parse error when `pyproject.toml` declares `[project.optional-dependencies]`.

- Fixed Python wheel installs with legacy metadata, whitespace around entry point targets, and interpreter options in script shebangs.

  Python dependency groups now match normalized names. Adding a development dependency reports when the project excludes the dev group.

  Comments ending in a backslash in `requirements.txt` no longer hide the next requirement. `pnpm exec` finds the environment of requirements-only projects under a tool-only `pyproject.toml`.

  Fixed `pnpm add` and `pnpm update` in shared Python workspaces that contain a requirements-only member.

- `pnpm install --frozen-lockfile` now fails when the `injected` setting of a dependency of a local directory dependency no longer matches the lockfile [pnpm/tasks#319](https://github.com/pnpm/tasks/issues/319).

- Approval prompts and notices now show a package from a named registry by its name. They no longer label it as a tarball.

- `pnpm-lock.yaml` now quotes strings that look like numbers, such as `+.5`, so they are read back as strings.

- `pnpm add` and `pnpm update` now keep the range operator of a specifier that has both a prerelease and a build suffix, such as `^1.0.0-next.0+build.1`.

- `pnpm install --frozen-lockfile` now revalidates registry metadata when verifying a locked tarball whose version is missing from the cached package listing.

- `pnpm install` now rejects local filesystem dependencies (`file:`, `link:`, and path-based `workspace:` specifiers) declared by non-local packages with `ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED`. Relative `file:` dependencies inside a package's own archive and local dependencies configured by the workspace through `overrides`, `packageExtensions`, or `.pnpmfile.cjs` continue to work. Lockfile verification also rejects unauthorized local directory, tarball, and link entries on non-importer snapshots.

- Fixed package maps resolving workspace and linked packages to the wrong directory when a path contains `#`, `?`, or `%`.

- Fixed `pnpm run` hanging after an interrupt that arrived while a script was starting. The script was never asked to stop, so pnpm waited for it for as long as it ran. pnpm no longer ends on such an interrupt without the script it had just started.

- Fixed `pnpm install` keeping a runtime binary for the previous platform after a change to `supportedArchitectures` with the global virtual store disabled.

- `pnpm remove` accepts Python and Cargo package specifiers. Recursive and filtered updates include standalone Python and Cargo workspace members. `pnpm update` and `pnpm outdated` match normalized Python names and Cargo aliases.

- Removed the obsolete `xmas` command.

- `pnpm install` now skips a Python project that has only a `requirements.txt` when its directory has a `.venv` that pnpm did not create, and prints a warning. The install failed for the whole workspace before.

- `--resume-from` starts at the named project when the previous run completed no tasks.

- `pnpm run` and `pnpm exec` no longer trigger a reinstall solely because the last install used `--trust-lockfile` [#18218](https://github.com/pnpm/pnpm/issues/18218).

- Fixed `pnpm update` and `pnpm outdated` with `--include-github-actions` matching every package dependency when combining a GitHub Action selector with a negated package selector such as `!lodash`.

- Tarball downloads send registry credentials only over HTTPS or loopback HTTP. Tarball and package metadata requests respect registry credential paths on redirects. A failed tarball request can no longer pin its redirect destination in the lockfile.

- A `[<since>]` filter such as `--filter "...[origin/main]"` now selects a project whose dependency an `overrides` entry points at a changed catalog entry. For example, with `lib: catalog:local` in `overrides`, changing `lib` in the `local` catalog selects every project that depends on `lib`.

- The pnpm binary is about 16% smaller on Linux and macOS.

- `pnpm sbom` now assigns workspace dependency relationships consistently across runs.

- `pnpm stage` uses the current package's publish registry when listing, viewing, approving, rejecting, or downloading staged versions. To use a one-time password at a non-default registry selected by the package, pass `--registry`.

- `pnpm store add` now fails if it cannot write a package to the store index, for example when another process holds the index locked. It also indexes tarball URLs that have no integrity value [pnpm/pnpm#18194](https://github.com/pnpm/pnpm/issues/18194).

- `pnpm store prune` now fails with `ERR_PNPM_FROZEN_STORE_WRITE` when `frozenStore` is enabled. It used to remove files from the store.

- `pnpm store prune` no longer removes files of the wheels that an installed Python project uses, so `pnpm install --offline` can recreate its environment. Pruning now also removes the store index entries of Python and Cargo archives whose files it deleted.

- Approving the `minimumReleaseAge` prompt no longer reprints the list of packages it just showed. pnpm now confirms with one line that says how many entries it added to `minimumReleaseAgeExclude` [#15377](https://github.com/pnpm/pnpm/issues/15377).

- After a workspace script runs, pnpm links commands from scoped injected packages into the containing modules directory's `.bin`. Commands declared through `directories.bin` are also refreshed.

- TLS file paths in `.npmrc` and `auth.ini` resolve relative to the file that declares them. Project `.npmrc` files skip TLS file paths containing environment variables.

  `pnpm config get` refuses to print protected auth settings. `pnpm config list` hides TLS private keys and inline proxy credentials. `pnpm config set --global tokenHelper` writes to `auth.ini`. Setting a project-level `tokenHelper` fails.

- The `nodeVersion` setting no longer changes the global virtual store directory names or the side-effects cache keys. These follow the Node.js runtime that pnpm installs for the project, then the `node` on `PATH`. `pnpm licenses` and `pnpm store status` now find packages that an install with `nodeVersion` placed in the global virtual store.

- When `pnpm unpublish` removes the version tagged `latest`, it prefers the highest remaining stable version.

- pnpm now fails with `ERR_PNPM_CONFIG_UNRESOLVED_ENV_VAR` when a setting in `pnpm-workspace.yaml` or the global `config.yaml` references an unset or empty environment variable without a fallback. A string setting used to get an empty string in its place [#10963](https://github.com/pnpm/pnpm/pull/10963).

- Fixed `pnpm update` and `pnpm outdated` matching every Cargo, Python, or npm dependency when combining a positive ecosystem selector with a negated package selector.

- A lockfile in the experimental `v13` format now always records `autoDedupe`, `dedupeInjectedDeps`, `dedupePeerDependents` and `linkWorkspacePackages`, as `lockfile.includeResolutionSettings` does for the `v9` format. After `pnpm install --frozen-lockfile` with `autoDedupe`, `pnpm run` no longer starts another install. Run `pnpm install` once to record the settings in a `v13` lockfile written by an earlier version, since `pnpm install --frozen-lockfile` fails on it [#16583](https://github.com/pnpm/pnpm/issues/16583).

- Reject unsupported types for the `noProxy` setting in workspace and global configuration. Strings and booleans remain valid, and `null` leaves the setting unset. Previously, unsupported values could reach proxy resolution without a validation error.

- `pnpm pack` and `pnpm publish` now reject `publishConfig.directory` values that use an absolute path or `..` segments outside the package directory.

- `pnpm version` rejects invalid prerelease identifiers passed to `--preid`.

- `pnpm version` now rejects a version argument that is not valid semver, such as `1.2.3foo` or `01.2.3`. It used to set a normalized version that differed from the argument.

- `pnpm version -r` updates explicit workspace dependency ranges that a release invalidates. `pnpm version` preserves edits that a `preversion` script makes to other manifest fields.

- Print a warning explaining why a modules directory has to be removed and recreated.

- pnpm in StackBlitz WebContainers now passes HTTP response bodies and subprocess output to its WebAssembly engine as raw bytes. A 64 KiB chunk no longer becomes a 234 KB JSON message [pnpm/tasks#63](https://github.com/pnpm/tasks/issues/63).

- `pnpm why --exclude-peers` now leaves out packages that are reached only as a resolved peer dependency. Previously the flag was accepted but ignored.

- On Windows, when the project is on a network share, `pnpm install` now links dependencies in `node_modules` with the kind of link that can be opened on that share. It tries a junction first and a symlink second. If neither can be opened, the install fails with advice instead of finishing with packages that cannot be loaded.

- Refreshing injected dependencies preserves the install's `NODE_PATH` settings in command shims and respects workspace package exclusions.

- `pnpm install` now links a workspace package for a `workspace:` range that combines `^`, `~`, or `*` with other ranges, such as `workspace:^ || ^2.0.0`.

- Updated dependencies:
  - get-pnpm@0.1.0
