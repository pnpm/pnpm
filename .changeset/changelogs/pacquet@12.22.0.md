## 12.22.0

### Minor Changes

- Settings under `globalPackagesConfig` in the global `config.yaml` now apply only to commands that act on global packages, such as `pnpm add -g` and `pnpm update -g`. For example, `globalPackagesConfig.trustPolicyExclude` excludes a package from the trust policy for global installs only [#18130](https://github.com/pnpm/pnpm/issues/18130).

  `pnpm approve-builds -g` and `pnpm add -g --allow-build` now save build decisions under `globalPackagesConfig.permissions`. pnpm still applies build settings from the `pnpm-workspace.yaml` of the global packages directory, with a deprecation warning. The next approval moves them to `config.yaml`.

- A `file:` dependency on a local directory now resolves `catalog:` specifiers in that directory's `package.json` against the workspace catalogs, as an injected workspace package does. Before, the install failed with `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`.

  A `file:` dependency on the `publishConfig.directory` of a project is now described by that project's `package.json`, as an injected workspace package is. A `package.json` that a build writes into that directory no longer changes the resolution.

- `pnpm list --recursive` now lists workspace projects in dependency order. Use `--no-sort` to retain workspace order [pnpm/pnpm#7716](https://github.com/pnpm/pnpm/issues/7716).

- The experimental `v13` lockfile format records a local directory package's declared specifiers in its `dependencies` and `optionalDependencies`, beside the versions they resolved to. A dependency marked `injected` in `dependenciesMeta` has `injected: true`. The format no longer has a `dependenciesMeta` field. Run `pnpm install` to rewrite a `v13` lockfile that an earlier pnpm wrote.

- Added `pnpm why --ranges`, which lists the version range that each direct dependent declares for the package. `pnpm why --json` now includes the declared range of each dependent in a `requires` field [pnpm/pnpm#18121](https://github.com/pnpm/pnpm/pull/18121).

### Patch Changes

- Fixed an offline install failing the `minimumReleaseAge` lockfile check with `ERR_PNPM_NO_OFFLINE_META` after an online install resolved the same packages with the same cache directory. The check now reads publish times from the metadata that the resolution cached [#18063](https://github.com/pnpm/pnpm/issues/18063).

- An error from a failed dependency resolution, such as an unreachable registry or a throwing pnpmfile resolver, now prints its cause once [#18077](https://github.com/pnpm/pnpm/issues/18077).

- pnpm no longer wraps error messages at the output width, so URLs and paths in them stay intact when stderr is not a terminal [#18078](https://github.com/pnpm/pnpm/issues/18078).

- `pnpm update` now moves an `engines.pnpm` range along with the pnpm version in `packageManager` or `devEngines.packageManager` [#18142](https://github.com/pnpm/pnpm/issues/18142).

- `verifyDepsBeforeRun` no longer reports the dependencies as out of date when `runtimeOnFail` stops pnpm from installing the runtime that `devEngines.runtime` or `engines.runtime` declares [#18155](https://github.com/pnpm/pnpm/issues/18155).

  Commands that install a single project of a workspace, such as `pnpm add -w`, now apply `runtimeOnFail` to the other workspace projects too. They no longer install a runtime that `runtimeOnFail` excludes.

- `pnpm audit` now reports advisories whose `id` is a string. Such advisories were dropped, and `pnpm audit` reported no known vulnerabilities [#18184](https://github.com/pnpm/pnpm/issues/18184).

- `pnpm install` and `pnpm approve` now discover, link, and prune agent skills from `workspace:*`, `link:`, and local directory `file:` direct dependencies.

- `pnpm approve-builds` and `pnpm add --allow-build` no longer corrupt a `pnpm-workspace.yaml` whose last line has no line break.

- `pnpm approve-builds` without package arguments or `--all` now fails with `ERR_PNPM_APPROVE_BUILDS_NON_INTERACTIVE` when packages are awaiting approval in CI or without an interactive terminal. `pnpm dlx` and global installs also skip automatic build-approval prompts in CI, leaving pending builds unapproved.

- `pnpm approve` without package arguments or `--all` now fails with `ERR_PNPM_APPROVE_NON_INTERACTIVE` when packages are awaiting approval in CI or without an interactive terminal.

- `pnpm add crate:` and targeted `pnpm update crate:` commands keep compatible versions from `Cargo.lock` for other crates [pnpm/tasks#257](https://github.com/pnpm/tasks/issues/257).

- Project `engines.node` checks now use the Node.js runtime that pnpm links for the dependency groups being installed. If the lockfile records that runtime, the check uses its resolved version. Production installs ignore a runtime declared only in `devEngines.runtime` [pnpm/tasks#302](https://github.com/pnpm/tasks/issues/302).

- `pnpm add crate:<pkg>` no longer corrupts multiline arrays in `Cargo.toml`. Saving an explicit version with `pnpm add crate:<pkg>@<version>` now replaces `workspace = true` on existing dependency entries. Inherited workspace settings such as `features`, `default-features`, and `package` stay on the updated entry. `pnpm install` now accepts unversioned path dependencies on prerelease Cargo workspace members. Cargo workspace inheritance now resolves `package` renames.

- `pnpm cat-file` now reports `ERR_PNPM_INVALID_HASH` when given an empty, malformed, or truncated digest such as `sha512-`.

- `pnpm ci` now validates that `pnpm-lock.yaml` exists and matches `package.json` before removing `node_modules`. `pnpm clean` and `pnpm ci` now honor `--filter`, `--filter-prod`, and `--workspace-root`. `pnpm clean` and `pnpm ci` also clean root `node_modules` in workspaces without a root `package.json`. `pnpm clean --lockfile` now removes per-project lockfiles and lockfiles inside a custom `lockfileDir`.

- `pnpm create` now forwards `--env-file` and other CLI flags placed after the starter template name to the starter command.

- `pnpm deploy` now preserves `lockfile.format` and `lockfileIncludeTarballUrl` in the deployed `pnpm-workspace.yaml`. Previously, commands such as `pnpm dedupe --check` in a deployed project using `lockfile.format: v13` treated the lockfile as `v9` and reported changes.

- `pnpm deploy --prod` now succeeds with `lockfile.format: v13` when a production package's optional peer is provided only by development dependencies. The excluded provider stays out of the deployed project [pnpm/pnpm#18117](https://github.com/pnpm/pnpm/issues/18117).

- `pnpm dlx` no longer applies `overrides`, `packageExtensions`, or `blockExoticSubdeps` from a local `pnpm-workspace.yaml` [#18075](https://github.com/pnpm/pnpm/pull/18075).

- `pnpm doctor` now checks write access for the cache, store, and global bin directories before they have been created. Previously, `pnpm doctor` reported `no write access` on fresh environments.

  `pnpm doctor` now probes whether reflinks or hardlinks work from the store into the project's link target and warns when neither does.

- When the `pnpm` package downloads its native binary on first run, it now reads a global `config.yaml` that holds only blank lines or comments as no settings. Such a file made every command fail with "expected a document, but the input is empty" [#18172](https://github.com/pnpm/pnpm/issues/18172).

- `pnpm -r fetch` now fetches each selected project's lockfile when `sharedWorkspaceLockfile` is `false`. Use `--filter` to limit which projects are fetched [pnpm/pnpm#18152](https://github.com/pnpm/pnpm/issues/18152).

- Combining `--filter` and `--filter-prod` with exclusion selectors such as `!pkg` now evaluates all selectors together in command-line order. An exclusion passed on one flag while inclusions were passed on the other no longer selects every workspace project.

- `pnpm clean`, `pnpm purge`, and `pnpm ci` keep files outside the project untouched when `modulesDir` points outside the project root. When `node_modules` is a symlink, `pnpm` now unlinks the symlink without modifying its target [#18072](https://github.com/pnpm/pnpm/pull/18072).

- `${VAR}` placeholders in `pnpm-workspace.yaml` written with YAML flow syntax now fail on registry URLs with `ERR_PNPM_REGISTRY_ENV_NOT_EXPANDED`. Placeholders in flow-style proxy, `pnprServer`, and `userAgent` settings are now ignored with a warning [#18074](https://github.com/pnpm/pnpm/pull/18074).

- `pnpm dlx` now keys its cache on active `overrides`, `packageExtensions`, `minimumReleaseAge`, `trustPolicy`, `trustLockfile`, and `blockExoticSubdeps` settings. Cached installs created under looser settings are no longer reused when stricter policies apply.

  `pnpm dlx --allow-build=!<pkg>` now denies build scripts for `<pkg>`, including the executed package itself. Denied packages also skip interactive build approval [#18075](https://github.com/pnpm/pnpm/pull/18075).

- `pnpm install` now re-verifies lockfile resolutions when the default registry or a scoped registry changes [#18070](https://github.com/pnpm/pnpm/pull/18070).

- `pnpm install` now verifies `pnpr` package revisions against `install-v2` documents and release records when the lockfile verification cache misses [#18070](https://github.com/pnpm/pnpm/pull/18070).

- When multiple `overrides` selectors match a dependency, pnpm now picks the selector with the narrower version range regardless of the order in which the overrides are declared [#18071](https://github.com/pnpm/pnpm/pull/18071).

- `pnpm --if-present start`, `pnpm run --if-present start`, and `pnpm restart --if-present` now exit with code 0 when a project has neither a `start` script nor a `server.js` file [#18073](https://github.com/pnpm/pnpm/pull/18073).

- `pnpm test`, `pnpm t`, and `pnpm tst` in a single-package project now fail with `ERR_PNPM_NO_SCRIPT` when the `test` script is missing unless `--if-present` is passed [#18073](https://github.com/pnpm/pnpm/pull/18073).

- `npm:<pkg>@<range>` and named-registry package aliases with space-separated comparators, such as `npm:bar@>=1.0.0 <2.0.0`, now resolve the target package [#18169](https://github.com/pnpm/pnpm/issues/18169).

- `pnpm env` commands (`use`, `list`, and `remove`) now accept Node.js version specifiers with a leading `v`, including prerelease versions such as `v22.0.0-rc.1`.

- Fixed GitHub Actions support in `pnpm update` and `pnpm outdated`. Updating a workflow step with a major or minor version comment such as `# v4` or a prose comment now formats the updated version comment cleanly. Negated action selectors such as `!actions/cache/save` now exclude only the targeted subpath actions. Passing only negated action selectors still updates packages. Previously, negated action selectors skipped package updates. `pnpm outdated` now reports every distinct outdated version when several workflows pin the same action at different versions.

- `pnpm update -g --latest` now upgrades global runtimes (`node`, `deno`, and `bun`) and `npm:`-aliased global packages across major versions.

- `finders` in `.pnpmfile.cjs` that return an empty string no longer match packages in `pnpm list --find-by` or `pnpm why --find-by`.

- `updateConfig` and `afterAllResolved` hooks in `.pnpmfile.cjs` that return `undefined`, `null`, or a non-object value now fail with `ERR_PNPM_CONFIG_IS_UNDEFINED` and `ERR_PNPM_BAD_AFTER_ALL_RESOLVED_RETURN`.

- Fixed `pnpm update` skipping package updates when a Package URL selector such as `pkg:npm/foo` is combined with `--include-github-actions`.

- `pnpm install` no longer runs `git ls-remote` for a git dependency whose specifier is unchanged and whose commit the lockfile already records. Installs that update the lockfile, such as Renovate's, no longer fail without git credentials when the project has a private git dependency with peer dependencies [#18104](https://github.com/pnpm/pnpm/issues/18104).

- `pnpm config set --global --json tools` now writes `tools` to the global `config.yaml` and validates its structure before saving. `pnpm config set --global` also accepts `pm-on-fail` and `runtime-on-fail`. `blockExoticSubdeps` and `excludeLinksFromLockfile` set in the global `config.yaml` now take effect during installs and appear in `pnpm config get --global`. pnpm now ignores workspace-only settings in the global `config.yaml` and prints a warning, matching `pnpm config set --global`.

- With `autoInstallPeers`, an optional peer is no longer bound to a version outside its range that only another dependency's subtree brings in. Before, adding an unrelated dependency could flip that binding and rewrite the peer suffixes of other projects in the lockfile [#18162](https://github.com/pnpm/pnpm/issues/18162).

- `pnpm install` with `nodeLinker: hoisted` no longer counts missing packages as removed. This fixes inflated removal counts for skipped optional dependencies [pnpm/pnpm#18178](https://github.com/pnpm/pnpm/issues/18178).

- `pnpm import` now preserves locked versions for `npm:` aliased dependencies when importing from `package-lock.json`, `npm-shrinkwrap.json`, or `yarn.lock`. `pnpm import` also parses Yarn Classic `yarn.lock` files that reference `__metadata` package names without treating them as Yarn Berry lockfiles.

- `pnpm lane <lane> --filter <selector>` now rejects partial moves of a `versioning.fixed` group with `ERR_PNPM_VERSIONING_CONFLICTING_CONFIG` before writing `pnpm-workspace.yaml`.

- `pnpm` warns when an `updateConfig` hook changes `registries` and some routes still use their configured registry. The warning names those routes and points to `registriesByScope`.

- `pnpm list` no longer adds an `npm:` prefix to aliased local `file:` directory dependencies, and `pnpm list --json` preserves `<name>@file:<path>` in their `version` field.

- `pnpm list --parseable --long` now keeps `<name>@` on `link:` and `file:` dependencies whose path or peer suffix contains `@`, such as `link:../@scope/pkg`. Aliased scoped `file:` dependencies whose path contains `@` also keep `<name>@` in `pnpm list`.

- Frozen installs accept injected workspace packages whose child dependencies use `link:` or a bare relative path [pnpm/tasks#274](https://github.com/pnpm/tasks/issues/274).

- `pnpm outdated --json` and `pnpm outdated --format=json` now write reporter warnings to `stderr`. Warnings previously appeared on `stdout` before the JSON output.

- `pnpm -r pack --out` now expands `%s` and `%v` placeholders only in the `--out` template. Literal `%s` and `%v` segments in the working directory path stay unchanged.

- Installing an injected local package no longer fails with a "no such file or directory" error when another process renames or removes files in that package while pnpm lists them. This happened most often on macOS in workspaces with `sharedWorkspaceLockfile: false`, where the install of the package itself writes its lockfile at the same time.

- Patches that add lines to the end of a file without a final newline now install the whole file. pnpm dropped the last added line when the patch also marked it with `\ No newline at end of file` [#18156](https://github.com/pnpm/pnpm/issues/18156).

- `pnpm peers` in workspaces with `sharedWorkspaceLockfile: false` now checks each selected project's `pnpm-lock.yaml`.

- `pnpm pipeline` now hashes no input files when `inputs: []` is configured. `+`-prefixed input globs include gitignored files and workspace files outside the project directory. `!`-prefixed input globs exclude matching files. Tasks that depend on an uncacheable task now bypass cached output restoration and re-run after that dependency executes. Read-only cached output files restore without permission errors. Task keys now hash each project's wanted lockfile across per-project lockfiles, custom `lockfileDir`, and branch lockfiles.

- `pnpm pkg` now reads and writes the project manifest as written on disk. `pnpm pkg get` no longer reports synthetic `runtime:` dependency entries from `devEngines.runtime` or `engines.runtime`. `pnpm pkg set` and `pnpm pkg delete` now preserve `devEngines.runtime`, `engines.runtime`, and empty dependency objects set with `--json`.

- `pnpm pm` now works after CLI flags such as `--dir` and after `pnpm with current`. `pnpm with current` also accepts subcommand boolean flags such as `pnpm --bare with current init` ahead of `with`.

- Fixed `pnpm install --merge-git-branch-lockfiles --frozen-lockfile` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a project sets or removes `publishConfig.directory`, `publishConfig.linkDirectory`, or `dependenciesMeta`. Lockfile merging now preserves and reconciles `publishDirectory`, `linkDirectory`, and `dependenciesMeta` on importers.

- `pnpm -C <dir> publish <tarball|folder>` and `pnpm --dir <dir> publish <tarball|folder>` now resolve relative tarball and directory arguments against `<dir>`. Git checks during `pnpm publish` also inspect the target directory.

- `pnpm add` for `pypi:` dependencies now preserves existing entries in `pyproject.toml` whose PEP 508 environment markers differ from the added requirement. `pnpm add` also accepts PEP 508 direct URL requirements such as `pypi:pkg @ https://...`. `pnpm update` at a shared-environment workspace root without a root `[project]` table now updates matching member manifests and the shared `pylock.toml`.

- An invalid entry in a Python registry's `packages` list now names the patterns a Python registry accepts: an exact name, a prefix such as `company-*`, or `**`. The error used to suggest `@scope/*`, which a Python registry rejects. It also quotes the entry as written. A rejected prefix such as `-a*` used to appear as `-ax`.

- `pnpm -r publish` now skips workspace manifests whose `name` or `version` field is an empty string.

- When the `pnpm` package downloads its native binary on first run, it now works with registries that serve no per-version package documents, such as Azure Artifacts. If such a registry also removed npm's signatures, pnpm fetches them from registry.npmjs.org. The binary still comes from the configured registry and is checked against npm's signature.

- `pnpm sbom` and `pnpm licenses list` now resolve `namedRegistries` version prefixes in lockfile entries. `pnpm sbom` emits bare component versions with a sanitized `repository_url` PURL qualifier. `pnpm sbom` derives tarball download URLs from the named registry. `pnpm licenses list` reports packages from named registries as separate entries with bare versions and `registryName`.

- `shellEmulator` now handles escaped backslashes (`\\`) and unquoted escaped single quotes (`\'`) according to POSIX quoting rules. Previously, scripts containing `"a\\"` or `\'` failed to parse or misquoted later `${...}` parameter expansions.

- `pnpm stage` with `--json` now keeps `stdout` valid JSON during dry runs, empty workspace runs, and errors. `pnpm pipeline --dry-run --json` and `pnpm -r run --dry-run --json` now write reporter warnings and scope lines to `stderr`.

- Tarball URLs that reject HEAD requests with 405 Method Not Allowed now fall back to a GET request during resolution.

- Fixed duplicate packages with `nodeLinker: hoisted` and `autoInstallPeers: false` when identical transitive peer bindings appeared under parents with conflicting dependencies.

- `pnpm update --changeset` now generates a changeset when a `.changeset` directory exists or `versioning` is configured in `pnpm-workspace.yaml`, without requiring `.changeset/config.json`. It also respects `versioning.ignore` alongside `.changeset/config.json`.

- Fixed a crash when `pnpm-lock.yaml` or a dependency selector contains a package name that starts with a multi-byte UTF-8 character.

- `pnpm install` with `lockfile.format: v13` no longer fails with `ERR_PNPM_NEXT_LOCKFILE_NOT_REPRESENTABLE` when it converts a `v9` lockfile in which a package with many peers has a hashed peer suffix and is in a dependency cycle with one of its peers [pnpm/pnpm#18148](https://github.com/pnpm/pnpm/issues/18148).

- `pnpm version -r` now runs each bumped package's `preversion`, `version`, and `postversion` scripts unless `--ignore-scripts` or `--dry-run` is set. `pnpm version -r --dry-run --json` now prints the planned releases as a JSON array.

- `pnpm why` no longer merges distinct local `file:` directory dependencies that share the same package name across projects with dedicated lockfiles (`sharedWorkspaceLockfile: false`).
