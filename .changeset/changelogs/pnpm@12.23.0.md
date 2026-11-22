## 12.23.0

This release adds `pnpm edit`, `pnpm catalog migrate`, and `pnpm link --hard`, and moves deprecation warnings into the install summary. Platform variants are removed, so packages that publish them need to move to `optionalDependencies`. It also carries security fixes: pnpm no longer reads `otp` from config files, and it sends registry credentials only over HTTPS.

### Minor Changes

#### Packing and publishing

- Removed platform variants. A package version has one archive again.

  pnpm no longer reads `publishConfig.variants` or the `variant` field of `package.json`. `pnpm pack --variant` and the `--upload-variant`, `--variant-integrity` and `--no-variants` options of `pnpm publish` are gone. pnpr no longer accepts variant archives with a version or serves them at `<name>@<version>/variants/<tag>`.

  To ship builds for several platforms, publish each as its own package and list them in `optionalDependencies`. A frozen install of a lockfile that records the variants of a registry package fails with `ERR_PNPM_BROKEN_LOCKFILE`. Run `pnpm install` to resolve the lockfile again.

- `pnpm pack` and `pnpm publish` now fail if a package that is not private lists a private workspace package in `dependencies`. A private package is never published, so the published package could not be installed. `devDependencies`, `optionalDependencies`, `peerDependencies`, and bundled dependencies are still allowed [#2321](https://github.com/pnpm/pnpm/issues/2321).

- `pnpm publish` now supports `--new-version <version>`, which sets the version in `package.json` before publishing. With `--recursive`, every selected workspace package is set to the new version [#2168](https://github.com/pnpm/pnpm/issues/2168).

- `pnpm pack --json` now reports the `package.json` that goes into the tarball, with `publishConfig` and the other publish-time changes applied. Run `pnpm pack --dry-run --json` to preview it without writing a tarball. A dry run still runs `prepack` and `prepare`, unless `--ignore-scripts` is passed.

- `pnpm pack` and `pnpm publish` now add a workspace-root `COPYING` file to a workspace package that has no license file. A `COPYING` file at the package root now counts as the package's license, so the workspace-root `LICENSE` is no longer added next to it.

- Added the `versioning.includePrivatePackages` setting. Set it to `false` to leave private packages out of `pnpm change` [#13813](https://github.com/pnpm/pnpm/issues/13813).

#### Managing dependencies

- Added the `pnpm edit` command to open an installed package directory in an editor.

- Added `pnpm catalog migrate`, which moves dependency specifiers into the default catalog and replaces them in the workspace projects' `package.json` files with `catalog:`. A dependency moves only when every project declares it with the same specifier. The command lists the dependencies it leaves unchanged [#7072](https://github.com/pnpm/pnpm/issues/7072).

- Added a `--hard` option to `pnpm link`. With `--hard`, pnpm saves the linked package with the `file:` protocol, so its files are hard linked into `node_modules` and its dependencies and peer dependencies resolve from your project. `pnpm unlink` removes these links too [#9895](https://github.com/pnpm/pnpm/issues/9895).

- `pnpm remove` now accepts glob patterns and `!` exclusions in dependency selectors, such as `pnpm remove "eslint-*" "!eslint-plugin-import"` [#13546](https://github.com/pnpm/pnpm/issues/13546).

- When pnpm updates a `package.json5`, it now changes only the values that changed. Comments, quotes, number formats, and layout elsewhere in the file stay as written. New keys and strings use the quote style of the file.

- `nodeLinker` accepts object settings for isolated and hoisted installs. Linker-specific hoisting options can be configured alongside the selected linker.

#### Updating dependencies

- `pnpm update --latest <pkg>` now moves the override that pins the named dependency [#8701](https://github.com/pnpm/pnpm/issues/8701). `pnpm update --interactive --latest` does the same for every selected dependency. An override keeps its own range shape: a pin on an exact version lands on the new exact version, and `^` stays `^`. When pnpm cannot update the governing override automatically, the update prints a warning naming it.

- `pnpm update` now accepts `--tag <tag>`. The flag updates the matched dependencies to the version behind the given dist-tag and rewrites the ranges in `package.json`, the way `--latest` updates to the `latest` tag. It also works with `--interactive` and with global updates [#3534](https://github.com/pnpm/pnpm/issues/3534).

- With `trustPolicy: no-downgrade`, `pnpm update --interactive` shows a `Trust` column with the trust evidence of each offered registry dependency [#8889](https://github.com/pnpm/pnpm/issues/8889). The list also offers only versions the trust policy accepts, so it matches what the update installs.

- `pnpm update --interactive --long` shows the homepage or repository URL of each package. The URL column is hidden without `--long`.

#### Release age and build settings

- Added the `referenceTime` setting. `minimumReleaseAge` and `trustPolicyIgnoreAfter` count back from this RFC 3339 time, both when pnpm resolves versions and when it verifies a lockfile. Set it in `pnpm-workspace.yaml` or with `PNPM_CONFIG_REFERENCE_TIME`.

- Added the `--minimum-release-age` and `--minimum-release-age-exclude` options to `pnpm install`, `pnpm add`, `pnpm update`, and `pnpm dlx`. They set `minimumReleaseAge` and `minimumReleaseAgeExclude` for a single run without editing a config file [#11224](https://github.com/pnpm/pnpm/issues/11224).

- `pnpm audit --fix` accepts `--no-add-release-age-exclusions`, which applies the fixes without adding entries to `minimumReleaseAgeExclude`.

- `--allow-build` now accepts a comma-separated list of packages, so `pnpm add foo bar --allow-build=foo,bar` is the same as passing `--allow-build=foo --allow-build=bar` [#9560](https://github.com/pnpm/pnpm/pull/9560).

#### Install output

- Deprecation warnings moved into the install summary and only name the packages the install added. A deprecated dependency added by the install is marked `deprecated` on its summary line, with the version that is not deprecated when there is one. Deprecated subdependencies the install added are listed right after the summary. Deprecated packages that were already installed are reported as one count, such as `3 deprecated packages are still installed`.

- In a workspace, the install summary now lists the direct dependency changes of every project, not only of the project pnpm runs in. Each change is listed once, followed by the projects that made it, unless only the current project did. A project's first install lists only deprecated or outdated dependencies and the ones the command added [#6481](https://github.com/pnpm/pnpm/issues/6481).

- `pnpm install` now warns when an entry in `overrides` matches no dependency, in a single line such as `2 overrides matched no dependency: foo, parent>child`. Only the overrides declared in the workspace are checked. Overrides added by a config dependency or an `updateConfig` hook are not. Set `warnUnusedOverrides: false` to turn the warning off [#10315](https://github.com/pnpm/pnpm/issues/10315).

- `pnpm install` now shows lockfile verification progress while the supply-chain check runs. The final pass or failure line reports how many entries were checked.

#### Setup and diagnostics

- `pnpm doctor` now checks the environment that lifecycle scripts run in. It lists the `node` executables on `PATH` and warns about broken ones. It reports the shell that runs scripts. A new check runs a package's executable from a script, both in a temporary project and in the current project. If that script fails, the report includes the error and, outside Windows, a trace of the executable's shim that shows how it looked up and started `node` [#16308](https://github.com/pnpm/pnpm/issues/16308).

- `pnpm self-update`, `pnpm setup`, and the "Install method" check of `pnpm doctor` now warn when another `pnpm` on `PATH` would run in place of the one pnpm installed. The warning names that copy. If pnpm recognizes how it was installed (npm, Homebrew, Corepack, Volta, or Scoop), the warning also gives the command that removes it.

- Set `PNPM_CONFIG_HOME` to choose the directory containing `config.yaml` and `auth.ini`. Global configuration commands read and write files in this directory. A non-empty value takes precedence over `XDG_CONFIG_HOME` and the platform defaults.

- pnpm now provides a prebuilt binary for Linux on LoongArch (loong64) with glibc.

- A failed `devEngines.runtime` or `engines.runtime` check now reports the reason in its error code. A runtime that is not installed fails with `ERR_PNPM_RUNTIME_NOT_FOUND`. A runtime entry without a version range fails with `ERR_PNPM_MISSING_RUNTIME_VERSION_RANGE`, and one with an invalid range fails with `ERR_PNPM_INVALID_RUNTIME_VERSION_RANGE`. An installed version that does not satisfy the range still fails with `ERR_PNPM_BAD_RUNTIME_VERSION`.

- `pnpm view <pkg>` now prints the registry it read the metadata from at the end of the summary [#11394](https://github.com/pnpm/pnpm/pull/11394).

### Patch Changes

#### Security

- `otp` is no longer read from any config file. Pass `--otp` on the command line, or set `PNPM_CONFIG_OTP`. A `pnpm-workspace.yaml` or a global `config.yaml` that carries `otp` is ignored, with a warning naming those two channels. `pnpm config set otp` fails with `ERR_PNPM_CONFIG_SET_NOT_A_FILE_SETTING`.

  This closes a way to leak a secret. A repository could write `otp: ${NPM_TOKEN}` beside its own `registry:`, and `pnpm publish` sent that environment variable to the registry the same file named, in the `npm-otp` header of its first request [#13542](https://github.com/pnpm/pnpm/issues/13542).

- pnpm now sends registry credentials only over HTTPS or loopback HTTP, both for tarball downloads and for requests to `pnprServer`. Tarball and package metadata requests respect registry credential paths on redirects. A failed tarball request can no longer pin its redirect destination in the lockfile.

- `pnpm install` now rejects local filesystem dependencies (`file:`, `link:`, and path-based `workspace:` specifiers) declared by non-local packages with `ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED`. Relative `file:` dependencies inside a package's own archive and local dependencies configured by the workspace through `overrides`, `packageExtensions`, or `.pnpmfile.cjs` continue to work. Lockfile verification also rejects unauthorized local directory, tarball, and link entries on non-importer snapshots.

- `pnpm install` and `pnpm install --frozen-lockfile` now enforce `blockExoticSubdeps` against entries in `pnpm-lock.yaml`. Before, pnpm accepted a lockfile with git or tarball URL subdependencies and installed those packages even with `blockExoticSubdeps: true`.

- `pnpm install` with `pnprServer` now rejects registry-keyed lockfile entries whose resolution uses a non-registry source with `ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`. The `pnpr` lockfile verification and resolution endpoints enforce the same check before accepting or caching a lockfile.

- `pnpm dlx`, `pnx`, and `pnpm create` now ignore local configuration in the working directory. This covers `.pnpmfile.cjs` hooks, proxy and TLS settings in the project `.npmrc`, and execution, layout, and resolution settings in a local `pnpm-workspace.yaml`, including `pnpmfile`, `globalPnpmfile`, `configDependencies`, `scriptShell`, `cacheDir`, `storeDir`, and `ignoreScripts`. Settings from the user's global `config.yaml`, such as `blockExoticSubdeps`, `cacheDir`, and `storeDir`, remain active.

- Global commands (`--global`, `-g`) no longer load `.pnpmfile.cjs`, `pnpm-workspace.yaml`, or `.npmrc` from the current working directory. `pnpm why -g` now inspects globally installed packages.

- `pnpm config get` refuses to print protected auth settings. `pnpm config list` hides TLS private keys and inline proxy credentials. `pnpm config set --global tokenHelper` writes to `auth.ini`, and setting a project-level `tokenHelper` fails.

  TLS file paths in `.npmrc` and `auth.ini` resolve relative to the file that declares them. Project `.npmrc` files skip TLS file paths containing environment variables.

- When the `pnpm` package downloads its native binary on first run, it now handles registry credentials the way pnpm does:

  - Credentials configured for a registry path, such as `//registry.example.com/org-a/npm/:_authToken`, are sent only to that path. They were sent to any path on the same host.
  - It reads the registry and credentials from the global `auth.ini`, where `pnpm login` writes them, and from the `_auth` setting of the global `config.yaml`.
  - A `${NPM_TOKEN?}` placeholder in `.npmrc` expands to the variable. It expanded to an empty value, so the download was sent without credentials.

- Matching a glob that repeats a wildcard, such as `+(*)z` in the workspace `packages` patterns, no longer hangs on long paths. The bundled picomatch is patched for a [security advisory](https://github.com/advisories/GHSA-fc5q-39xc-vjpf).

- `pnpm pack` and `pnpm publish` now reject `publishConfig.directory` values that use an absolute path or `..` segments outside the package directory.

- The `pnpm` binary no longer depends on `chrono`, which RustSec reports as unmaintained in RUSTSEC-2026-0543.

#### Installing packages

- `pnpm install` now adds a new workspace package that has no dependencies to the `importers` of `pnpm-lock.yaml` [#18189](https://github.com/pnpm/pnpm/issues/18189).

- With `enableGlobalVirtualStore` on, `require.extensions` hooks run again for `require()` calls from a module compiled from a string, such as the module Next.js uses to load `next.config.ts`. A required `.ts` file skipped the registered TypeScript hook. A required `.json` file was parsed as JavaScript [#18261](https://github.com/pnpm/pnpm/issues/18261).

- On Windows, when the project is on a network share, `pnpm install` now links dependencies in `node_modules` with the kind of link that can be opened on that share. It tries a junction first and a symlink second. If neither can be opened, the install fails with advice.

- On Windows, concurrent installs that link the same dependency no longer leave an `.ignored_<name>` entry next to the link in the global virtual store.

- `pnpm install` now fails with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a `file:` dependency that is not a workspace project uses a `catalog:` specifier. Injected workspace projects still resolve their `catalog:` dependencies.

- A `link:` or relative-path dependency of a local directory dependency now resolves from that directory. In any other package, such as a local tarball, it resolves from the lockfile directory. Both used to resolve from the importing project's directory [pnpm/tasks#319](https://github.com/pnpm/tasks/issues/319).

  Two workspace projects that declare the same injected relative-path dependency, such as `../child`, no longer share the directory the first one resolved.

- `pnpm install` now links a workspace package for a `workspace:` range that combines `^`, `~`, or `*` with other ranges, such as `workspace:^ || ^2.0.0`.

- Fixed `pnpm install` keeping a runtime binary for the previous platform after a change to `supportedArchitectures` with the global virtual store disabled.

- The `nodeVersion` setting no longer changes the global virtual store directory names or the side-effects cache keys. These follow the Node.js runtime that pnpm installs for the project, then the `node` on `PATH`. `pnpm licenses` and `pnpm store status` now find packages that an install with `nodeVersion` placed in the global virtual store.

- Fixed package maps resolving workspace and linked packages to the wrong directory when a path contains `#`, `?`, or `%`.

- `pnpm install` and `pnpm peers check` no longer report a `catalog:` peer of an injected workspace package as unmet when the installed version matches the catalog entry [#10279](https://github.com/pnpm/pnpm/issues/10279).

- The peer dependency check now reads the version of an injected workspace package from its `package.json`. `pnpm install` with `strictPeerDependencies: true` and `pnpm peers check` no longer report such a peer as unmet with a `file:` version.

- With `sharedWorkspaceLockfile: false`, `pnpm install` now reports missing peers of linked workspace packages, as `pnpm peers check` does.

- `peerDependencyRules.allowedVersions` now accepts a parent version range that contains `>`, such as `"foo@>=1.0.0>bar": "1"`.

- Fixed `nodeLinker: hoisted` installs in a workspace where the directory of one project is inside the directory of another. A dependency of the inner project could resolve to the version in the outer project's `node_modules`. pnpm now keeps the inner project's dependency in its own `node_modules` when the outer project holds another version.

- With `nodeLinker: hoisted`, pnpm now finds the agent skills of a dependency whose directory is shared with another peer variant of the same package version.

- `pnpm install` with `pnprServer` set now handles more workspace settings:

  - It applies `injectWorkspacePackages`, `dedupeInjectedDeps`, `ignoredOptionalDependencies`, and `dependenciesMeta`. Projects that use them no longer fail with `ERR_PNPM_OUTDATED_LOCKFILE` or `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH`.
  - `pnpm install --update-checksums` refreshes the integrity checksums in the lockfile.
  - It enforces `minimumReleaseAgeStrict` and adds fallback versions to `minimumReleaseAgeExclude` when strict mode is off.
  - It resolves locally when the project declares `file:`, `link:`, or local path dependencies, or when `blockExoticSubdeps` is `false`. `pnpr` also rejects local filesystem dependency specifiers on `/-/pnpr/v0/resolve`.

- When an overrides entry requests a version that does not exist on the registry, `pnpm install` now names that override entry in the error. If the entry's selector has a version range, such as `foo@^1`, the error also shows the latest published version in that range.

- `pnpm add` run from a workspace package directory that has no `package.json` yet now suggests creating one with `pnpm init` [#11345](https://github.com/pnpm/pnpm/issues/11345).

- The install summary now shows "(X is available)" after a direct dependency whose `latest` version on the registry is newer than the one installed. The note appears when the install resolved that dependency against registry metadata.

- Approving the `minimumReleaseAge` prompt no longer reprints the list of packages it just showed. pnpm now confirms with one line that says how many entries it added to `minimumReleaseAgeExclude` [#15377](https://github.com/pnpm/pnpm/issues/15377).

- pnpm now prints a warning that explains why it has to remove and recreate a modules directory.

- `pnpm install` with `nodeLinker: hoisted` no longer hangs or runs out of memory in large workspaces whose packages are installed with many different sets of peer dependencies.

- Sped up `pnpm install` on macOS for packages installed with several sets of peer dependencies. pnpm now clones every peer variant of a package from one cached copy of its files.

- Sped up linking bins in large workspaces. pnpm now reuses a workspace package's `package.json` across the projects that depend on it when linking their bins.

- `pnpm install --frozen-lockfile` no longer waits for a Node.js version check it does not use before it exits. With Homebrew's Node.js, a frozen install with nothing to do can be about 3x faster.

- Resolving a remote tarball uses the download response directly, saving a request.

- pnpm in StackBlitz WebContainers sends HTTP response bodies and subprocess output to its engine as raw bytes. A 64 KiB chunk no longer becomes a 234 KB message [pnpm/tasks#63](https://github.com/pnpm/tasks/issues/63).

#### Lockfile

- Fixed `pnpm install` failing with `ERR_PNPM_NEXT_LOCKFILE_NOT_REPRESENTABLE` under `lockfile.format: v13` when a peer dependency is provided by an aliased package and another alias installs the package that has the peer's name [#18198](https://github.com/pnpm/pnpm/issues/18198).

- Fixed writing v13 lockfiles when a package has an ordinary npm alias of one of its direct or transitive peers [#18274](https://github.com/pnpm/pnpm/issues/18274).

- Fixed conversion to the v13 lockfile format when a hashed peer suffix includes a workspace link and the package installs its own copy of another declared peer [#18288](https://github.com/pnpm/pnpm/issues/18288).

- `pnpm install` now converts an existing lockfile when you change `lockfile.format`, even when all dependencies are already installed. With `lockfile.format: v13`, an install that updates a v9 lockfile without resolving, for example after a dependency is removed, now writes the v13 format.

- A lockfile in the experimental `v13` format now always records `autoDedupe`, `dedupeInjectedDeps`, `dedupePeerDependents` and `linkWorkspacePackages`, as `lockfile.includeResolutionSettings` does for the `v9` format. After `pnpm install --frozen-lockfile` with `autoDedupe`, `pnpm run` no longer starts another install. Run `pnpm install` once to record the settings in a `v13` lockfile written by an earlier version, since `pnpm install --frozen-lockfile` fails on it [#16583](https://github.com/pnpm/pnpm/issues/16583).

- `pnpm install --frozen-lockfile` now checks the dependencies of local directory dependencies against the lockfile more closely. It fails when the `injected` setting of such a dependency no longer matches the lockfile [pnpm/tasks#319](https://github.com/pnpm/tasks/issues/319). It accepts a plain-range dependency that `linkWorkspacePackages: deep` linked to a workspace project.

- Frozen installs now detect changes to catalog entries used only by peer dependencies of local directory packages.

- Frozen installs now restore links declared through catalogs when `excludeLinksFromLockfile` is enabled.

- `pnpm install --frozen-lockfile` now revalidates registry metadata when verifying a locked tarball whose version is missing from the cached package listing.

- During dependency filtering in commands such as `pnpm install --prod --frozen-lockfile`, pnpm now recovers hashed peers in v9 lockfiles only when a binding can affect which dependencies are kept [#18276](https://github.com/pnpm/pnpm/issues/18276).

- Fixed `pnpm install --lockfile-only` fetching a locked transitive Git dependency when an unrelated dependency changes [#18217](https://github.com/pnpm/pnpm/issues/18217).

- Fixed `pnpm fetch` failing when a Docker build copies the lockfile before the workspace configuration or pnpmfile.

- `pnpm-lock.yaml` now quotes strings that look like numbers, such as `+.5`, so they are read back as strings.

#### Build approvals

- `pnpm approve-builds --all` and `pnpm approve --all` no longer approve a package whose build is denied with `false` in `pnpm-workspace.yaml`.

- A `<pkg>@<version>: false` entry in `allowBuilds` or `permissions` now also blocks `<pkg>@<version>` from a named registry, including when `dangerouslyAllowAllBuilds` is set.

- `pnpm add --allow-build` now records its decision under `permissions` when `pnpm-workspace.yaml` has a `permissions` setting. Before, a `--allow-build=!<pkg>` denial went to `allowBuilds` and was overridden by an existing `permissions.<pkg>.build: true`.

- `pnpm approve-builds` now requires the exact selector shown by `pnpm ignored-builds` for a dependency a bare package name cannot identify (a `file:`, git, or tarball artifact). A bare package name only approves a plain registry version [#18237](https://github.com/pnpm/pnpm/issues/18237).

- `pnpm approve-builds`, `pnpm approve`, and `pnpm add --allow-build` now refuse to edit a package's `permissions` entry written as an inline mapping that spans several lines. Before, they added a second key for the package to `pnpm-workspace.yaml`.

- Install now reports packages awaiting build and skill approval in one notice. Approval prompts and notices show a package from a named registry by its name. They no longer label it as a tarball.

#### Running scripts and tasks

- Fixed `pnpm run` hanging after an interrupt:

  - A script may leave behind a process that ignores the interrupt. pnpm now waits thirty seconds for the script's processes to stop and then exits, leaving any that are still running.
  - An interrupt that arrives while a script is starting now reaches that script. pnpm no longer waits for it for as long as it runs, or ends without it.
  - When pnpm cancels a script, it now waits up to ten seconds for the system's process list. A process that a cancelled script started is no longer left running on a machine that answers more slowly than half a second.

- `pnpm run` and `pnpm exec` no longer trigger a reinstall solely because the last install used `--trust-lockfile` [#18218](https://github.com/pnpm/pnpm/issues/18218).

- The workspace project graph now follows `overrides`. An override that points a dependency at a workspace package, such as `foo: workspace:*`, makes that package a dependency of every project whose declaration the override matches, even with `linkWorkspacePackages` off. `pnpm install` now runs that package's lifecycle scripts before the scripts of the projects depending on it. `pnpm -r run` orders the projects the same way. `--filter` selectors that follow dependencies now see the edge.

- A `[<since>]` filter such as `--filter "...[origin/main]"` now selects a project whose dependency an `overrides` entry points at a changed catalog entry. For example, with `lib: catalog:local` in `overrides`, changing `lib` in the `local` catalog selects every project that depends on `lib`.

- When `sharedWorkspaceLockfile` is `false`, `pnpm install` now runs the workspace root's `pnpm:devPreinstall` script once, before any project is installed, including when a filter excludes the root. Workspace members no longer run theirs [#4503](https://github.com/pnpm/pnpm/issues/4503).

- Recursive runs honor `--sequential` with `--parallel`, and changed-file ignore patterns support negation. `pnpm list -r --no-sort --reverse` reverses the selected project order. `pnpm clean` forwards positional arguments to a project's clean script.

- `--resume-from` starts at the named project when the previous run completed no tasks.

- `pnpm pipeline` fixes:

  - Cached results are invalidated when values supplied by `--env-file`, `extraEnv`, or `NODE_OPTIONS` change, or when the script shell settings change.
  - Declared outputs of sibling tasks no longer invalidate the default inputs, and output globs support `!` exclusions.
  - It respects `--filter` and `--filter-prod`, includes untracked files when selecting affected projects, and runs the prerequisites of tasks matched by regex selectors.
  - It no longer deletes files from a build script's output directory when it restores a shared Cargo build cache, such as a `fingerprint` or `.fingerprint` directory in a nested Cargo build.

- With `shellEmulator: true`, parameter expansions such as `${MODE:-dev}` now see assignments made earlier in the script.

- Plug'n'Play fixes:

  - It respects package exports and `require.resolve()` lookup paths.
  - It combines custom export conditions from `NODE_OPTIONS` and command-line flags, and respects Node.js's enabled `node-addons` and `module-sync` conditions.
  - `pnpm run` and `pnpm exec`, including recursive runs, find the loader and the experimental package map when `lockfileDir` is configured.
  - `pnpm install` recreates a deleted `.pnp.cjs` loader, and a full `pnpm clean` removes the generated loader.

- After a workspace script runs, pnpm links commands from scoped injected packages into the containing modules directory's `.bin`. Commands declared through `directories.bin` are also refreshed.

- Refreshing injected dependencies preserves the install's `NODE_PATH` settings in command shims and respects workspace package exclusions.

- pnpm recognizes uppercase file extensions when choosing the interpreter for commands without a shebang. It skips hidden files and directories when linking commands from `directories.bin`.

- `pnpm dlx`, `pnx`, and `pnpm create` now handle script skipping and non-registry package specifiers consistently:

  - Running with `--ignore-scripts` or `ignoreScripts: true` uses a separate `dlx` cache key, keeping cached installs that skipped build scripts separate from normal runs.
  - Relative local directory and tarball specifiers such as `./my-cli` and `./my-cli.tgz` resolve against the caller's working directory.
  - Explicitly requested tarball URL, `file:`, and git URL packages run their build scripts by default unless `--ignore-scripts` or a negated `--allow-build` selector disables them.

- A `tasks` entry with no value in `pnpm-workspace.yaml`, such as `build:`, now fails with a hint. The hint suggests writing `{}` for a task with no settings, or removing the entry to keep the default `dependsOn: ['^<task>']`.

#### Python and Cargo

- `pnpm install` now locks Cargo path dependencies outside the workspace, as Cargo does. It failed with `workspace dependency <name> is not a workspace member`. `pnpm install --frozen-lockfile` now also fails when `Cargo.lock` misses a path dependency or locks a git dependency at another branch, tag, or revision.

- `pnpm outdated` and `pnpm update` now include Cargo dependencies declared under `[target.<cfg>.dependencies]`. `pnpm update --prod` no longer fails on a crate that is also a dev-dependency. `pnpm add crate:` fails in a virtual Cargo workspace root, as `cargo add` does. It added a `[dependencies]` table that Cargo rejects there.

- `pnpm remove` accepts Python and Cargo package specifiers. Recursive and filtered updates include standalone Python and Cargo workspace members. `pnpm update` and `pnpm outdated` match normalized Python names and Cargo aliases.

- `pnpm install` now skips a Python project that has only a `requirements.txt` when its directory has a `.venv` that pnpm did not create, and prints a warning. The install failed for the whole workspace before.

- Fixed Python wheel installs with legacy metadata, whitespace around entry point targets, and interpreter options in script shebangs.

  Python dependency groups now match normalized names. Adding a development dependency reports when the project excludes the dev group.

  Comments ending in a backslash in `requirements.txt` no longer hide the next requirement. `pnpm exec` finds the environment of requirements-only projects under a tool-only `pyproject.toml`.

  Fixed `pnpm add` and `pnpm update` in shared Python workspaces that contain a requirements-only member.

- Fixed `pnpm update` failing with a TOML parse error when `pyproject.toml` declares `[project.optional-dependencies]`.

- `pnpm store prune` no longer removes files of the wheels that an installed Python project uses, so `pnpm install --offline` can recreate its environment. Pruning now also removes the store index entries of Python and Cargo archives whose files it deleted.

#### Updating, auditing, and publishing

- `pnpm audit` now checks tarball, git, and named-registry dependencies against security advisories. `pnpm audit signatures` now verifies packages installed from named registries and skips non-registry dependencies.

- `pnpm audit` now checks patched versions correctly when registry metadata includes an `unpublished` entry in its `time` field [#18209](https://github.com/pnpm/pnpm/issues/18209).

- `pnpm audit --fix=true` now applies the default override fix method, like a bare `pnpm audit --fix` [#13261](https://github.com/pnpm/pnpm/issues/13261).

- `pnpm outdated` now reports newer prereleases of an installed prerelease. It checks the dist-tag that points to the newest version of the installed prerelease channel, such as `next` for `3.0.0-rc.0`, alongside `latest` [#7339](https://github.com/pnpm/pnpm/issues/7339).

- Fixed `pnpm update` and `pnpm outdated` matching every dependency of an ecosystem when a positive selector is combined with a negated package selector such as `!lodash`. This applied to Cargo, Python, npm, and `--include-github-actions` selectors.

- Fixed `pnpm update` and `pnpm outdated` with `--include-github-actions` corrupting workflow YAML or skipping actions referenced through YAML anchors, aliases, or merge keys.

- `pnpm add` and `pnpm update` now keep the range operator of a specifier that has both a prerelease and a build suffix, such as `^1.0.0-next.0+build.1`.

- `pnpm dedupe --check` no longer requests the registry metadata of locked versions that the cache already holds. `pnpm dedupe` skips those requests too when `lockfile.format` is `v13`, which records no `deprecated` notices. Both were slow against a registry that sends no `ETag` or `Last-Modified`, such as Azure Artifacts, where each request downloaded the full package metadata [#17758](https://github.com/pnpm/pnpm/issues/17758).

- `pnpm publish` now reads `access` and `tag` from `pnpm-workspace.yaml`, the global `config.yaml`, and `PNPM_CONFIG_ACCESS` / `PNPM_CONFIG_TAG`. The `--access` and `--tag` flags still win when set. A configured `access` outranks `publishConfig.access`. An empty `publishBranch` now allows `master` and `main` [#13542](https://github.com/pnpm/pnpm/issues/13542).

- `pnpm pack` and `pnpm publish` fixes:

  - Packed workspace dependencies use the dependency's `publishConfig.name`.
  - Relative workspace dependencies with a `package.yaml` or `package.json5` manifest are packed correctly, and these packages include their parked changelog.
  - The manifest returned by `beforePacking` is validated and normalized.

- `pnpm version` fixes:

  - It rejects a version argument that is not valid semver, such as `1.2.3foo` or `01.2.3`. It used to set a normalized version that differed from the argument.
  - It rejects invalid prerelease identifiers passed to `--preid`.
  - `pnpm version -r` updates explicit workspace dependency ranges that a release invalidates.
  - It preserves edits that a `preversion` script makes to other manifest fields.

- When `pnpm unpublish` removes the version tagged `latest`, it prefers the highest remaining stable version.

- `pnpm stage` uses the current package's publish registry when listing, viewing, approving, rejecting, or downloading staged versions. To use a one-time password at a non-default registry selected by the package, pass `--registry`.

- Fixed several bugs in patching dependencies:

  - A patch that only changes a file's mode no longer corrupts a binary file.
  - A patch with CRLF line endings now applies when a blank context line has lost its leading space.
  - A unified diff without `diff --git` lines now applies.
  - A patch that creates a symlink now fails with `ERR_PNPM_INVALID_PATCH`. Before, pnpm wrote the symlink as a regular file.
  - `pnpm patch-commit --patches-dir ../outside` now fails with `ERR_PNPM_PATCHES_DIR_OUTSIDE_PROJECT`. Before, pnpm wrote the patch to `outside` inside the project.
  - `pnpm patch-commit` keeps a file without a final newline unterminated when that file comes last in the patch. Before, the patched file gained a trailing newline.
  - `pnpm patch-commit` keeps the peers a patched package is bound to in the lockfile. It no longer fails with `ERR_PNPM_NEXT_LOCKFILE_NOT_REPRESENTABLE` when the package has an optional peer and `lockfile.format` is `v13`.
  - A name-only `patchedDependencies` entry now records its patch hash on a `file:` directory dependency or an injected workspace package. `pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_INCONSISTENT_PATCH_HASH` on the lockfile that `pnpm install` wrote.
  - `pnpm patch` and `pnpm patch-commit` find a package installed from a `namedRegistries` alias by its version. For example, `pnpm patch foo@1.0.0` patches a dependency declared as `work:1.0.0`.
  - `pnpm patch-remove` removes the corresponding edit directory and state entry from `node_modules/.pnpm_patches`.

#### Other commands

- Global packages fixes:

  - `pnpm add -g` with several space-separated packages lists every installed package in its summary. Only the packages of the first one were listed.
  - `pnpm list -g` with `--depth` greater than `0` always reads the global install directory's `node_modules` and lockfile. It could read the current project's lockfile or a custom `modulesDir`.
  - `pnpm add -g`, `pnpm update -g`, and `pnpm outdated -g` use the `_auth` and `npmrcAuthFile` settings under `globalPackagesConfig` in the global `config.yaml`.
  - Commands that act on global packages honor `dangerouslyAllowAllBuilds` set at the top level of the global `config.yaml`. pnpm ignored it for global packages unless it was set under `globalPackagesConfig`.

- Fixed `pnpm deploy` for projects that depend on an external directory through `link:`. `pnpm deploy` now also resolves relative `file:` overrides from the source workspace [#18191](https://github.com/pnpm/pnpm/issues/18191).

- Fixed `pnpm link` for target manifests without a name. The command uses the directory name and normalizes relative paths written to the manifest. It also resolves a quoted `~/` path from the home directory.

- Fixed `pnpm prune` leaving extraneous package links in hoisted installations, and `pruneDirectDependencies` skipping cleanup when the lockfile was already up to date.

- `pnpm import` fixes:

  - It writes each project's lockfile when `sharedWorkspaceLockfile` is disabled and `lockfileDir` is unset.
  - It respects `disallowWorkspaceCycles` and the symlink policy for the destination lockfile.
  - It normalizes workspace-relative Yarn patch paths, skips Yarn patches outside the workspace, and accepts Yarn Classic entries identified by a URL or file path.

- `pnpm store add` now fails if it cannot write a package to the store index, for example when another process holds the index locked. It also indexes tarball URLs that have no integrity value [#18194](https://github.com/pnpm/pnpm/issues/18194).

- `pnpm store prune` now fails with `ERR_PNPM_FROZEN_STORE_WRITE` when `frozenStore` is enabled. It used to remove files from the store.

- Store and cache inspection fixes:

  - `pnpm cache list`, `view`, and `delete` search across registries by default and recognize mixed-case package names. Pass `--registry` to restrict them to one registry.
  - `pnpm cat-file` can read executable files from the store.
  - `pnpm find-hash` reports an unknown hash when the store is empty.
  - Inspection commands with `--json` write errors to stdout.
  - `pnpm clean` removes custom virtual stores with hidden directory names.

- `pnpm list --lockfile-only --find-by` now reads manifests from the configured store when `node_modules` is absent, and `pnpm licenses list` reads cached package metadata and license files.

- `pnpm list --json` now reports unsaved dependencies from each workspace project's own modules directory [#18248](https://github.com/pnpm/pnpm/issues/18248).

- `pnpm why --exclude-peers` now leaves out packages that are reached only as a resolved peer dependency. The flag was accepted but ignored.

- `pnpm sbom` now assigns workspace dependency relationships consistently across runs.

- pnpm now fails with `ERR_PNPM_CONFIG_UNRESOLVED_ENV_VAR` when a setting in `pnpm-workspace.yaml` or the global `config.yaml` references an unset or empty environment variable without a fallback. A string setting used to get an empty string in its place [#10963](https://github.com/pnpm/pnpm/pull/10963).

- `--config.<name>` options now validate their values. `--config.storeDir` and `--config.stateDir` work like `--store-dir` and `--state-dir`. An empty `--state-dir=` resets to the default state directory, as `--store-dir=` does.

- `noProxy` in `pnpm-workspace.yaml` or the global `config.yaml` now fails validation unless it is a string or a boolean. `null` leaves the setting unset.

- `pnpm doctor` checks the resolved global bin directory on `PATH`.

- `pnpm --version` now succeeds when the project pins another package manager.

- Fixed pnpm dropping Ctrl+C just before launching its native binary through Corepack or Node.js on Unix [#18222](https://github.com/pnpm/pnpm/issues/18222).

- The pnpm npm package provides bundled node-gyp to lifecycle scripts even when pnpm's own install scripts were skipped [#18227](https://github.com/pnpm/pnpm/issues/18227).

- The pnpm binary is about 16% smaller on Linux and macOS.

- Tab completion suggests project scripts when you type `pnpm` followed by a script name [#18239](https://github.com/pnpm/pnpm/issues/18239). Shell completion uses the cursor position when editing an existing command.

- The help for `pnpm config get` and `pnpm get` now says that the key is a property path, such as `catalog.react`.

- Removed the obsolete `xmas` command.
