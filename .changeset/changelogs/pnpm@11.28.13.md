## 11.28.13

### Patch Changes

- Fixed an offline install failing the `minimumReleaseAge` lockfile check with `ERR_PNPM_NO_OFFLINE_META` after an online install resolved the same packages with the same cache directory. The check now reads publish times from the metadata that the resolution cached [#18063](https://github.com/pnpm/pnpm/issues/18063).

- `pnpm audit` now reports advisories whose `id` is a string. Such advisories were dropped, and `pnpm audit` reported no known vulnerabilities [#18184](https://github.com/pnpm/pnpm/issues/18184).

- pnpm no longer stalls on a patch file whose `diff --git` line contains a long run of spaces. `@pnpm/patching.apply-patch` no longer depends on `@pnpm/patch-package`.

- `pnpm approve-builds` without package arguments or `--all` now fails with `ERR_PNPM_APPROVE_BUILDS_NON_INTERACTIVE` when packages are awaiting approval in CI or without an interactive terminal. `pnpm dlx` and global installs also skip automatic build-approval prompts in CI, leaving pending builds unapproved.

- Project `engines.node` checks now use the Node.js runtime that pnpm links for the dependency groups being installed. If the lockfile records that runtime, the check uses its resolved version. Production installs ignore a runtime declared only in `devEngines.runtime` [pnpm/tasks#302](https://github.com/pnpm/tasks/issues/302).

- `pnpm cat-file` now reports `ERR_PNPM_INVALID_HASH` when given an empty, malformed, or truncated digest such as `sha512-`.

- `pnpm ci` now validates that `pnpm-lock.yaml` exists and matches `package.json` before removing `node_modules`. `pnpm clean` and `pnpm ci` now honor `--filter`, `--filter-prod`, and `--workspace-root`. `pnpm clean` and `pnpm ci` also clean root `node_modules` in workspaces without a root `package.json`. `pnpm clean --lockfile` now removes per-project lockfiles and lockfiles inside a custom `lockfileDir`.

- `pnpm install` now refreshes local dependencies when a catalog range no longer accepts its locked peer version.

- `pnpm doctor` now checks write access for the cache, store, and global bin directories before they have been created. Previously, `pnpm doctor` reported `no write access` on fresh environments.

  `pnpm doctor` now probes whether reflinks or hardlinks work from the store into the project's link target and warns when neither does.

- `pnpm -r fetch` now fetches each selected project's lockfile when `sharedWorkspaceLockfile` is `false`. Use `--filter` to limit which projects are fetched [pnpm/pnpm#18152](https://github.com/pnpm/pnpm/issues/18152).

- Combining `--filter` and `--filter-prod` with exclusion selectors such as `!pkg` now evaluates all selectors together in command-line order. An exclusion passed on one flag while inclusions were passed on the other no longer selects every workspace project.

- `pnpm clean`, `pnpm purge`, and `pnpm ci` keep files outside the project untouched when `modulesDir` points outside the project root. When `node_modules` is a symlink, `pnpm` now unlinks the symlink without modifying its target [#18072](https://github.com/pnpm/pnpm/pull/18072).

- `pnpm dlx` now keys its cache on active `overrides`, `packageExtensions`, `minimumReleaseAge`, `trustPolicy`, `trustLockfile`, and `blockExoticSubdeps` settings. Cached installs created under looser settings are no longer reused when stricter policies apply.

  `pnpm dlx --allow-build=!<pkg>` now denies build scripts for `<pkg>`, including the executed package itself. Denied packages also skip interactive build approval [#18075](https://github.com/pnpm/pnpm/pull/18075).

- `pnpm install` now re-verifies lockfile resolutions when the default registry or a scoped registry changes [#18070](https://github.com/pnpm/pnpm/pull/18070).

- When multiple `overrides` selectors match a dependency, pnpm now picks the selector with the narrower version range regardless of the order in which the overrides are declared [#18071](https://github.com/pnpm/pnpm/pull/18071).

- `pnpm --if-present start`, `pnpm run --if-present start`, and `pnpm restart --if-present` now exit with code 0 when a project has neither a `start` script nor a `server.js` file [#18073](https://github.com/pnpm/pnpm/pull/18073).

- `pnpm env` commands (`use`, `list`, and `remove`) now accept Node.js version specifiers with a leading `v`, including prerelease versions such as `v22.0.0-rc.1`.

- Fixed GitHub Actions support in `pnpm update` and `pnpm outdated`. Updating a workflow step with a major or minor version comment such as `# v4` or a prose comment now formats the updated version comment cleanly. Negated action selectors such as `!actions/cache/save` now exclude only the targeted subpath actions. Passing only negated action selectors still updates packages. Previously, negated action selectors skipped package updates. `pnpm outdated` now reports every distinct outdated version when several workflows pin the same action at different versions.

- `pnpm update -g --latest` now upgrades global runtimes (`node`, `deno`, and `bun`) and `npm:`-aliased global packages across major versions.

- `updateConfig` and `afterAllResolved` hooks in `.pnpmfile.cjs` that return `undefined`, `null`, or a non-object value now fail with `ERR_PNPM_CONFIG_IS_UNDEFINED` and `ERR_PNPM_BAD_AFTER_ALL_RESOLVED_RETURN`.

- pnpm now reports `globalPackagesConfig` in the global `config.yaml` as a setting of a newer pnpm version. It no longer suggests a similarly named setting.

- With `autoInstallPeers`, an optional peer is no longer bound to a version outside its range that only another dependency's subtree brings in. Before, adding an unrelated dependency could flip that binding and rewrite the peer suffixes of other projects in the lockfile [#18162](https://github.com/pnpm/pnpm/issues/18162).

- `pnpm install` with `nodeLinker: hoisted` no longer counts missing packages as removed. This fixes inflated removal counts for skipped optional dependencies [pnpm/pnpm#18178](https://github.com/pnpm/pnpm/issues/18178).

- `pnpm import` now preserves locked versions for `npm:` aliased dependencies when importing from `package-lock.json`, `npm-shrinkwrap.json`, or `yarn.lock`. `pnpm import` also parses Yarn Classic `yarn.lock` files that reference `__metadata` package names without treating them as Yarn Berry lockfiles.

- `pnpm install` resolves the `link:` and bare relative path dependencies of an injected workspace package from that package's directory. They were resolved from the lockfile directory, so the installed link could point outside the workspace [pnpm/tasks#274](https://github.com/pnpm/tasks/issues/274).

- `pnpm lane <lane> --filter <selector>` now rejects partial moves of a `versioning.fixed` group with `ERR_PNPM_VERSIONING_CONFLICTING_CONFIG` before writing `pnpm-workspace.yaml`.

- `pnpm` warns when an `updateConfig` hook changes `registries` and some routes still use their configured registry. The warning names those routes and points to `registriesByScope`.

- `pnpm list --parseable --long` now keeps `<name>@` on `link:` and `file:` dependencies whose path or peer suffix contains `@`, such as `link:../@scope/pkg`. Aliased scoped `file:` dependencies whose path contains `@` also keep `<name>@` in `pnpm list`.

- Frozen installs accept injected workspace packages whose child dependencies use `link:` or a bare relative path [pnpm/tasks#274](https://github.com/pnpm/tasks/issues/274).

- `pnpm outdated --json` and `pnpm outdated --format=json` now write reporter warnings to `stderr`. Warnings previously appeared on `stdout` before the JSON output.

- `pnpm pack` now accepts `--ignore-pnpmfile` and `--ignore-scripts`, and `pnpm publish` accepts `--ignore-pnpmfile`. With these flags, a package can be packed without running the project's pnpmfile or lifecycle scripts [#18132](https://github.com/pnpm/pnpm/issues/18132).

- `pnpm pack` and `pnpm publish` in a workspace now keep the workspace's ignore rules when a package's `.npmignore` is removed while pnpm lists the package's files.

- `pnpm -r pack --out` now expands `%s` and `%v` placeholders only in the `--out` template. Literal `%s` and `%v` segments in the working directory path stay unchanged.

- Installing an injected local package no longer fails with a "no such file or directory" error when another process renames or removes files in that package while pnpm lists them. This happened most often on macOS in workspaces with `sharedWorkspaceLockfile: false`, where the install of the package itself writes its lockfile at the same time.

- Patches that add lines to the end of a file without a final newline now install the whole file. pnpm dropped the last added line when the patch also marked it with `\ No newline at end of file` [#18156](https://github.com/pnpm/pnpm/issues/18156).

- `pnpm peers` in workspaces with `sharedWorkspaceLockfile: false` now checks each selected project's `pnpm-lock.yaml`.

- `pnpm pkg` now reads and writes the project manifest as written on disk. `pnpm pkg get` no longer reports synthetic `runtime:` dependency entries from `devEngines.runtime` or `engines.runtime`. `pnpm pkg set` and `pnpm pkg delete` now preserve `devEngines.runtime`, `engines.runtime`, and empty dependency objects set with `--json`.

- `pnpm pm` now works after CLI flags such as `--dir` and after `pnpm with current`. `pnpm with current` also accepts subcommand boolean flags such as `pnpm --bare with current init` ahead of `with`.

- Fixed `pnpm install --merge-git-branch-lockfiles --frozen-lockfile` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a project sets or removes `publishConfig.directory`, `publishConfig.linkDirectory`, or `dependenciesMeta`. Lockfile merging now preserves and reconciles `publishDirectory`, `linkDirectory`, and `dependenciesMeta` on importers.

- `pnpm -C <dir> publish <tarball|folder>` and `pnpm --dir <dir> publish <tarball|folder>` now resolve relative tarball and directory arguments against `<dir>`. Git checks during `pnpm publish` also inspect the target directory.

- Tarball URLs that reject HEAD requests with 405 Method Not Allowed now fall back to a GET request during resolution.

- `pnpm update --changeset` now generates a changeset when a `.changeset` directory exists or `versioning` is configured in `pnpm-workspace.yaml`, without requiring `.changeset/config.json`. It also respects `versioning.ignore` alongside `.changeset/config.json`.

- `pnpm version -r` now runs each bumped package's `preversion`, `version`, and `postversion` scripts unless `--ignore-scripts` or `--dry-run` is set. `pnpm version -r --dry-run --json` now prints the planned releases as a JSON array.

- `pnpm why` no longer merges distinct local `file:` directory dependencies that share the same package name across projects with dedicated lockfiles (`sharedWorkspaceLockfile: false`).
