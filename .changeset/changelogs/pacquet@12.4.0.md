## 12.4.0

### Minor Changes

- pnpm now runs on Android, on arm64 and x64 [#14431](https://github.com/pnpm/pnpm/issues/14431).

- pnpm now runs on FreeBSD systems with x64 CPUs [#14597](https://github.com/pnpm/pnpm/issues/14597). It also runs on Linux systems with ppc64le and s390x CPUs.

- pnpm now runs on Linux systems with RISC-V (riscv64) CPUs that use glibc [#7582](https://github.com/pnpm/pnpm/issues/7582).

- pnpm can install Cargo dependencies from the sparse registry configured by `cargo.indexUrl`. The generated `Cargo.lock` records that registry as the source of every crate.

- `pnpm install` and `pnpm add crate:<package>` now manage crates.io dependencies when `cargo.enabled` is set in `pnpm-workspace.yaml`. Mixed-ecosystem workspaces install their dependency graphs in one command. A single `pnpm add` command can include both npm packages and crates. Cargo workspace discovery excludes configured stores and caches.

  Cargo lockfiles and source configuration are published after all enabled ecosystems finish successfully. Failed publication restores the previous Cargo metadata. Failed `pnpm add` operations that include crates restore participating manifests and lockfiles.

  Cargo index and crate downloads use URL-matched credentials from pnpm configuration. The crates.io index also supports `CARGO_REGISTRY_TOKEN` and the token in `$CARGO_HOME/credentials.toml`, using Cargo's bare `Authorization` header format. Cargo registry requests respect the configured fetch retry budget.

- `pnpm install` now resolves Cargo dependencies through the server configured in `pnprServer`. The client no longer fetches one sparse-index file per crate in the dependency graph. If the server does not serve Cargo resolution, pnpm resolves Cargo dependencies locally.

- Added `pnpm change check`. It validates the committed package versions against the `versioning.epics` bands and the `versioning.fixed` groups in `pnpm-workspace.yaml` and lists every violation. It is meant to run in CI, because `pnpm version -r` only checks the packages it releases.

- `pnpm pipeline` can reuse local Cargo build state between worktrees with the `tasks.<name>.cargoTargetDir` setting. Restored build directories remain usable after the cache is deleted. Root tasks can participate with `includeWorkspaceRoot: true`.

- Added `pnpm pipeline [name]` to install frozen dependencies and run workspace tasks declared in `pipelines`. It selects affected projects and runs their task graph without bailing on task failures.

  Task settings now support `outputs`, `inputs`, `env`, and `cache`. Cache hits restore declared outputs and replay the task's logs.

  `pnpm pipeline --dry-run` previews the task graph without installing configuration dependencies or executing workspace hooks.

- `pnpm install` and `pnpm add pypi:<package>` now manage Python wheel dependencies when `python.enabled` is set in `pnpm-workspace.yaml`. Python dependencies use `pyproject.toml`, the standard `pylock.toml` lockfile, and a managed `.venv`. Frozen and offline installs are supported. `pnpm run` and `pnpm exec` include that environment's executables.

  Mixed-ecosystem installs wait for all enabled ecosystems before publishing Python state. Failed publication restores Python metadata and the previous environment. Python workspace discovery excludes configured stores and caches.

  Network and archive retry logs hide credentials and signed query parameters in request URLs.

  [pnpm/pnpm#14566](https://github.com/pnpm/pnpm/issues/14566)

- `pnpm install` now resolves Python dependencies through the server configured in `pnprServer`. The client no longer downloads a wheel to find out what it requires. If the server does not serve Python resolution, pnpm resolves Python dependencies locally.

- Added a new setting `trustPolicyExcludePrune` (default: `false`). When enabled, `pnpm add`, `pnpm update`, and `pnpm remove` prune the entries of `trustPolicyExclude` in `pnpm-workspace.yaml` that the freshly written lockfile no longer resolves: versions that are gone are dropped (an entry is removed once none of its versions remain), and entries for packages that are no longer in the lockfile are removed too. Name patterns (`@scope/*`) are always kept. The cleanup is skipped when the install's lockfile does not cover the whole workspace (`sharedWorkspaceLockfile: false`), since entries another project still needs would look stale.

### Patch Changes

- `pnpm add --workspace <pkg>` works again. It saves the package under the `workspace:` protocol and links it from the workspace. It fails when no workspace project provides the package. pnpm v12 had rejected the flag as an unknown argument [#14602](https://github.com/pnpm/pnpm/issues/14602).

- Build scripts can now be rejected before the package is installed [#14067](https://github.com/pnpm/pnpm/issues/14067):

  - `pnpm add --allow-build=!<pkg>` records `<pkg>: false` in `allowBuilds`. It used to write a `!<pkg>: true` entry that matched no package. On a global install the denial was dropped altogether, so the post-install prompt offered the build for approval.
  - `pnpm approve-builds <pkg>` and `pnpm approve-builds !<pkg>` record their decision even when no packages are awaiting approval, and report a package that is not awaiting approval with a warning so a typo stays visible. Both cases used to fail the command with an error.

- `pnpm audit` no longer counts advisories suppressed by `auditConfig` in its summary. The headline total and the `Severity:` breakdown now count only the advisories that survive the `ignoreGhsas` filter. Suppressed advisories get their own line, `2 ignored: 1 moderate | 1 critical`. A run whose advisories were all suppressed printed a red `1 vulnerabilities found` next to a zero exit code. It now reads `All found vulnerabilities were already reviewed and decided to be ignored` [#14535](https://github.com/pnpm/pnpm/issues/14535).

- Boolean flags now accept an explicit value written inline. `pnpm install --prod=false` installs devDependencies, and `--prod=true` skips them [#14553](https://github.com/pnpm/pnpm/issues/14553).

- A relative `scriptShell` path in `pnpm-workspace.yaml` is now resolved against the workspace root, so scripts run from a nested workspace package find the shell [#14422](https://github.com/pnpm/pnpm/issues/14422). A bare command name such as `bash` is still looked up on `PATH`.

- `pnpm deploy` now runs the source workspace's pnpmfile. It used to run the copy of that pnpmfile that the deploy leaves in the target directory. That failed the deploy with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` for any project that ships a `.pnpmfile.mjs` [#14671](https://github.com/pnpm/pnpm/issues/14671).

- Sped up `pnpm deploy` in workspaces with many projects [#14539](https://github.com/pnpm/pnpm/issues/14539).

- Fixed pnpm failing to provision the version pinned in `packageManager` when the project sets `nodeLinker: hoisted`. Provisioning a managed Node.js, Deno, or Bun runtime now ignores a `nodeLinker: hoisted` in the global config too [#14595](https://github.com/pnpm/pnpm/issues/14595).

- The JavaScript pnpm can again switch to the pnpm version a project pins in `packageManager` on hosts where the native pnpm build ships no binary, such as Alpine Linux with pnpm 10 or an Intel Mac with pnpm 11 [#13622](https://github.com/pnpm/pnpm/issues/13622).

  When the pnpm build being switched to is native and ships no binary for the host, pnpm now names the host target it lacks. pnpm reported that the binary was missing from `pnpm-lock.yaml`.

- Fixed `pnpm install` failing with `ERR_PNPM_LOCKFILE_IS_SYMLINK` in a project with config dependencies when `pnpm-lock.yaml` is a symlink, as build sandboxes such as Bazel and Nix stage it. pnpm no longer rewrites the lockfile when the recorded config dependencies are unchanged. Writing changed config dependencies through a symlinked lockfile is still refused. A lockfile that starts with a byte order mark now keeps its main document when its config dependencies are updated [#14372](https://github.com/pnpm/pnpm/issues/14372).

- pnpm now keeps archive extraction within its concurrency limit when an install abandons a download whose extraction is still running. The extraction slot was released as soon as the install stopped waiting for it, letting the next extraction start too early [#14585](https://github.com/pnpm/pnpm/issues/14585).

- `fetch-timeout` now limits how long a request may make no progress. The timer restarts on every chunk that arrives. A large download over a slow connection is no longer aborted while data is still coming in. A connection that stops delivering data still fails after `fetch-timeout` [#14604](https://github.com/pnpm/pnpm/issues/14604).

- Workspace package patterns such as `./packages/*` now match. Exclusions such as `!./packages/foo` apply too. pnpm normalizes `.` segments, `..` segments, and repeated slashes in the `packages` field of `pnpm-workspace.yaml` before matching them [#14571](https://github.com/pnpm/pnpm/issues/14571).

- Fixed `pnpm install --frozen-lockfile` rejecting a lockfile that pnpm had just generated. The rejection happened when `pnpm.overrides` pointed a dependency at a relative `file:` or `link:` path and the workspace had projects below the lockfile directory [#14555](https://github.com/pnpm/pnpm/issues/14555).

- Fixed concurrent installs that share a global virtual store on macOS failing with "failed to import ... No such file or directory" while another install was populating the same package [#14560](https://github.com/pnpm/pnpm/issues/14560).

- `pnpm install` failed with `Invalid CA certificate` when a `ca` or `cafile` setting carried something pnpm could not read as a certificate. The unreadable certificate is now ignored and the readable ones around it still apply. A `cert` or `key` setting with a blank value now reads as unset [#14646](https://github.com/pnpm/pnpm/issues/14646).

- `pnpm import` now leaves the project-local lockfile unchanged when `lockfileDir` points to another directory. Failed imports restore the destination lockfile. Imports that use a branch lockfile leave the shared lockfile unchanged [#14563](https://github.com/pnpm/pnpm/issues/14563).

- `pnpm install <pkg>` now accepts `--offline` and `--prefer-offline`. These flags already worked with `pnpm add <pkg>`, but the install spelling failed with `unexpected argument` [#14194](https://github.com/pnpm/pnpm/pull/14194).

- Fixed `pnpm setup` failing with `ERR_PNPM_DIRECTORY_FETCHER_PATH_ESCAPE` on Windows [#14618](https://github.com/pnpm/pnpm/issues/14618). A `file:` dependency whose directory is a symlink or junction is now packed from the directory it points at. Files that a symlink inside the package reaches outside that directory are still refused.

- `pnpm install` no longer fails with `ERR_PNPM_META_FETCH_FAIL` on a lockfile that holds a JSR dependency. pnpm reads `@jsr/*` metadata from npm.jsr.io. The supply-chain policy check asked the default registry for it and got a 404 [#14649](https://github.com/pnpm/pnpm/issues/14649).

- Commands run from a subdirectory of a project now act on the nearest ancestor directory that has a manifest, as they do on pnpm 11. `pnpm bin` printed a `node_modules/.bin` path under the current directory, which does not exist [#14622](https://github.com/pnpm/pnpm/issues/14622). `pnpm init` still creates its `package.json` in the current directory, and `pnpm exec` still runs its command there.

- A `registry` or `@scope:registry` set in an `.npmrc` now wins over the registry a `pnpm login` credential stored in the global `config.yaml` points at. Previously, after logging in to one registry, installs in a project whose `.npmrc` named a private registry went to the logged-in registry instead. They now go to the registry the `.npmrc` names [#14614](https://github.com/pnpm/pnpm/issues/14614).

- `pnpm outdated -r` now wraps the `Dependents` column at 30 columns. A dependency used by many workspace projects listed all of them on one line, which made the table hundreds of columns wide [#14591](https://github.com/pnpm/pnpm/issues/14591).

- `packageConfigs` settings now reach the projects they name. In a workspace where each project keeps its own lockfile (`sharedWorkspaceLockfile: false`), an entry's `overrides`, `hoist`, `modulesDir`, `saveExact`, and `savePrefix` apply to that project's install. A workspace that shares one lockfile still ignores these entries, and the install now says which ones it ignored [#14556](https://github.com/pnpm/pnpm/issues/14556).

- `pnpm pack --json` now reports errors as JSON. Lifecycle script stdout and stderr appear before the final JSON output.

- A range whose upper bound leaves out a component, such as `<=16` or `<=2.0`, now matches every version that component stands for. `<=16` matched no 16.x version at all, so a peer dependency declared `>=0.11 <=3` was reported unmet by 3.0.1 [#14419](https://github.com/pnpm/pnpm/issues/14419).

- `pnpm patch-commit` now writes a valid patch when a file is added to or deleted from the edit directory. `pnpm install` now applies patches that delete a file without listing its contents. Deleting a file made the next install fail with `ERR_PNPM_INVALID_PATCH` [#14559](https://github.com/pnpm/pnpm/issues/14559).

- A patch that gives a dependency a `preinstall`, `install`, or `postinstall` script, or a `binding.gyp`, now runs that build. pnpm asks for build approval first, so the package is listed under "Ignored build scripts" until it is allowed to build. pnpm 12 ran nothing, and pnpm 11 ran it without asking [#14648](https://github.com/pnpm/pnpm/issues/14648).

- Registries that share a host but differ by URL path — one JFrog Artifactory, Nexus, AWS CodeArtifact or GitLab Packages instance serving several repositories — now get a metadata cache directory each. Previously they shared one, so resolving a package from one of them could answer with another's versions, integrity hashes and tarball URLs and fail with `ERR_PNPM_TARBALL_URL_MISMATCH` [#13558](https://github.com/pnpm/pnpm/issues/13558).

  The URL scheme is part of the cache directory name too, so an `http` registry can no longer hand its metadata — which can be rewritten in transit — to a resolution configured for `https` at the same host.

  The first install after upgrading refetches registry metadata once. The package store is untouched.

  `pnpm cache view` now labels each entry with the full registry URL. It printed `registry.npmjs.org` before and prints `https://registry.npmjs.org/` now.

  `pnpm cache list-registries` and `pnpm cache list` print the new directory names. Scripts that parse either command need updating.

- `pnpm pipeline` now rejects symlinked inputs when computing task cache keys. Unreadable input files now produce an error. Non-UTF-8 input filenames now produce an error. Unix filenames containing backslashes are now hashed as literal paths.

  Projects inside or containing Git submodules and tasks depending on them bypass caching and still execute.

- Tab-completing `pn` now lists the same commands and flags as `pnpm`. The scripts printed by `pnpm completion` register the `pn` alias for bash, fish, pwsh, and zsh [#11955](https://github.com/pnpm/pnpm/issues/11955).

- `pnpm add` and `pnpm install` now accept protocol-prefixed selectors such as `jsr:@scope/pkg`, `npm:pkg@^1.0.0`, and `workspace:pkg@*`. The package name spelled inside the selector keys the manifest entry. A `jsr:` request is saved with the picked version pinned, so `pnpm add jsr:@scope/pkg` records `jsr:^1.2.3`. These selectors used to fail with `ERR_PNPM_INVALID_DEPENDENCY_NAME` [pnpm/pnpm#14590](https://github.com/pnpm/pnpm/issues/14590).

- `pnpm install` now applies patch files with CRLF line endings [pnpm/pnpm#14557](https://github.com/pnpm/pnpm/issues/14557).

- pnpm now records the pinned pnpm version in the lockfile the project actually uses when `lockfileDir` is set. It wrote the pin into a second `pnpm-lock.yaml` at the workspace root, and the real lockfile never carried it [#14633](https://github.com/pnpm/pnpm/issues/14633).

- pnpm no longer rewrites the `packageManagerDependencies` block of `pnpm-lock.yaml` back and forth when package manager version switching is turned off. `pnpm install` recorded the pinned pnpm version there and commands such as `pnpm list` did not [#14575](https://github.com/pnpm/pnpm/issues/14575).

- On Windows, pnpm now retries removing and replacing command shims that another process has temporarily locked during installation [#14549](https://github.com/pnpm/pnpm/issues/14549).

- Fixed argument forwarding on Windows when `shellEmulator` is enabled. Paths ending in a backslash, line breaks, and literal shell expressions are preserved [pnpm/pnpm#14548](https://github.com/pnpm/pnpm/issues/14548).

- `pnpm run` and `pnpm exec` now check only the project they run in when `sharedWorkspaceLockfile` is false. `verifyDepsBeforeRun` reported a changed workspace structure on every run in such a workspace, even directly after a successful install [#14588](https://github.com/pnpm/pnpm/issues/14588).

- `pnpm version` now accepts `-m` as a short alias for `--message` [#14567](https://github.com/pnpm/pnpm/issues/14567).

- Sped up restoring a workspace with many projects from a warm global virtual store. Linking a project's bins no longer rereads dependency manifests that the store index already holds [#14540](https://github.com/pnpm/pnpm/issues/14540).

- On Windows, `pnpm store path` now prints a path that uses backslashes throughout. The `storeDir` and `virtualStoreDir` values recorded in `node_modules/.modules.yaml` use backslashes as well. These paths previously carried forward slashes in the middle, so they did not match what pnpm 11 writes for the same directory.

- Resolving Yarn 6 authenticates its GitHub releases-API request with `GH_TOKEN` / `GITHUB_TOKEN` when one is set. The release list is the one unconditional GitHub API call the resolver makes, and the anonymous rate limit is counted per IP address — which CI runners share — so provisioning `yarn@6` in CI could fail with `ERR_PNPM_YARN_RELEASES_STATUS` no matter how rarely a single job asked. Exporting the token CI already has lifts the request onto the authenticated limit. The token is withheld when the project turns off `strict-ssl`, so relaxing certificate verification for a registry never sends a GitHub credential over the connection it relaxed.
