## 12.4.0

### Minor Changes

- pnpm can now manage npm, Python, and Cargo dependencies in the same workspace. Enable `python.enabled` or `cargo.enabled` in `pnpm-workspace.yaml`, then use `pnpm install` to install them together.

  - Add Python packages with `pnpm add pypi:<package>`. pnpm uses `pyproject.toml`, `pylock.toml`, and a managed `.venv`. Frozen and offline installs are supported, and `pnpm run` and `pnpm exec` make the environment's executables available [#14566](https://github.com/pnpm/pnpm/issues/14566).
  - Add Rust crates with `pnpm add crate:<package>`. pnpm supports crates.io and custom sparse registries configured with `cargo.indexUrl`. Registry authentication supports pnpm credentials and, for crates.io, `CARGO_REGISTRY_TOKEN` or `$CARGO_HOME/credentials.toml`.

  Both ecosystems support faster dependency resolution through `pnprServer`, with local resolution as a fallback when the server does not support it.

- Added `pnpm pipeline [name]` to install frozen dependencies and run workspace tasks declared in `pipelines`. It selects affected projects, runs their task graph, and continues running tasks after a task fails.

  Tasks support `inputs`, `outputs`, `env`, and `cache` settings. Cached results restore task outputs and replay logs. Cargo tasks can reuse local build state between worktrees with `tasks.<name>.cargoTargetDir`. Set `includeWorkspaceRoot: true` to include root tasks.

  Use `pnpm pipeline --dry-run` to preview the task graph without installing configuration dependencies or running workspace hooks.

- Added support for Android on arm64 and x64, FreeBSD on x64, and Linux on ppc64le, s390x, and RISC-V (riscv64 with glibc) [#14431](https://github.com/pnpm/pnpm/issues/14431), [#14597](https://github.com/pnpm/pnpm/issues/14597), [#7582](https://github.com/pnpm/pnpm/issues/7582).

- Added `trustPolicyExcludePrune` to automatically remove unused versions and packages from `trustPolicyExclude` when running `pnpm add`, `pnpm update`, or `pnpm remove`. It is disabled by default. Package name patterns such as `@scope/*` are kept, and cleanup is skipped when `sharedWorkspaceLockfile` is `false`.

- Added `pnpm change check` for CI validation of package versions against the `versioning.epics` bands and `versioning.fixed` groups in `pnpm-workspace.yaml`. It reports all violations, including packages that are not part of the current release.

### Patch Changes

- Registry metadata is now kept separate for registries with different URL paths or schemes. This prevents installs from using another registry's package versions or tarball URLs, and keeps metadata fetched over HTTP from being reused for HTTPS [#13558](https://github.com/pnpm/pnpm/issues/13558).

  The first install after upgrading refetches registry metadata. The package store is unchanged. `pnpm cache view` now shows full registry URLs. Scripts that parse the directory names from `pnpm cache list-registries` or `pnpm cache list` need updating.

- Patches that add build scripts or a `binding.gyp` now trigger a build, subject to build approval. Unapproved builds appear under "Ignored build scripts" [#14648](https://github.com/pnpm/pnpm/issues/14648).

- Build scripts can now be rejected before installing a package with `pnpm add --allow-build=!<pkg>`, including global installs. `pnpm approve-builds <pkg>` and `pnpm approve-builds !<pkg>` also save decisions when no packages are awaiting approval. They warn if the named package is not awaiting approval [#14067](https://github.com/pnpm/pnpm/issues/14067).

- A registry configured in `.npmrc` now takes precedence over registry settings saved by `pnpm login` in the global `config.yaml`. This fixes installs using the wrong registry after login [#14614](https://github.com/pnpm/pnpm/issues/14614).

- Large downloads over slow connections no longer time out while data is still arriving. `fetch-timeout` now limits how long a request can go without making progress [#14604](https://github.com/pnpm/pnpm/issues/14604).

- Sped up installs in workspaces with many projects when reusing a warm global virtual store [#14540](https://github.com/pnpm/pnpm/issues/14540).

- `pnpm deploy` is faster in large workspaces and no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when the project includes a `.pnpmfile.mjs` [#14539](https://github.com/pnpm/pnpm/issues/14539), [#14671](https://github.com/pnpm/pnpm/issues/14671).

- `pnpm add --workspace <pkg>` works again. It saves the dependency with the `workspace:` protocol and links it from the workspace. The command fails if no workspace project provides the package [#14602](https://github.com/pnpm/pnpm/issues/14602).

- `pnpm add` and `pnpm install` now accept protocol-prefixed selectors such as `jsr:@scope/pkg`, `npm:pkg@^1.0.0`, and `workspace:pkg@*` [#14590](https://github.com/pnpm/pnpm/issues/14590). Installs with JSR dependencies in the lockfile also no longer fail with `ERR_PNPM_META_FETCH_FAIL` [#14649](https://github.com/pnpm/pnpm/issues/14649).

- Boolean flags now accept explicit inline values. For example, `pnpm install --prod=false` installs devDependencies, while `--prod=true` skips them [#14553](https://github.com/pnpm/pnpm/issues/14553).

- `pnpm install <pkg>` now accepts `--offline` and `--prefer-offline`, as `pnpm add <pkg>` already did [#14194](https://github.com/pnpm/pnpm/pull/14194).

- Fixed `pnpm install --frozen-lockfile` rejecting a freshly generated lockfile when overrides use relative `file:` or `link:` paths in a workspace [#14555](https://github.com/pnpm/pnpm/issues/14555).

- Fixed installs with config dependencies failing on symlinked lockfiles, such as those used by Bazel and Nix, when the config dependencies have not changed. Updates that would write through a symlink remain disallowed. Updating config dependencies also preserves lockfiles that start with a byte order mark [#14372](https://github.com/pnpm/pnpm/issues/14372).

- Fixed package manager version pins being written to the wrong lockfile when `lockfileDir` is set. The pins also remain consistent across commands when version switching is disabled, avoiding unnecessary lockfile changes [#14633](https://github.com/pnpm/pnpm/issues/14633), [#14575](https://github.com/pnpm/pnpm/issues/14575).

- `pnpm import` now respects `lockfileDir` and branch lockfiles without modifying other lockfiles. Failed imports restore the destination lockfile [#14563](https://github.com/pnpm/pnpm/issues/14563).

- `pnpm patch-commit` now produces valid patches when files are added or deleted. `pnpm install` also accepts patches that delete files without listing their contents, and patch files with CRLF line endings [#14559](https://github.com/pnpm/pnpm/issues/14559), [#14557](https://github.com/pnpm/pnpm/issues/14557).

- Fixed version ranges with partial upper bounds. For example, `<=16` now includes all 16.x versions, and `>=0.11 <=3` correctly accepts 3.0.1 [#14419](https://github.com/pnpm/pnpm/issues/14419).

- Workspace package patterns now support `.` and `..` segments and repeated slashes. Patterns such as `./packages/*` and exclusions such as `!./packages/foo` now match correctly [#14571](https://github.com/pnpm/pnpm/issues/14571).

- `packageConfigs` settings now apply to the specified projects when `sharedWorkspaceLockfile` is `false`, including `overrides`, `hoist`, `modulesDir`, `saveExact`, and `savePrefix`. Workspaces with a shared lockfile report which entries were ignored [#14556](https://github.com/pnpm/pnpm/issues/14556).

- `pnpm run` and `pnpm exec` no longer report a changed workspace structure after a successful install when `sharedWorkspaceLockfile` is `false` and `verifyDepsBeforeRun` is enabled [#14588](https://github.com/pnpm/pnpm/issues/14588).

- Commands run from a project's subdirectory now find the nearest ancestor with a manifest. This fixes commands such as `pnpm bin` returning paths under the wrong directory. `pnpm init` still creates its manifest in the current directory, and `pnpm exec` still runs there [#14622](https://github.com/pnpm/pnpm/issues/14622).

- Relative `scriptShell` paths in `pnpm-workspace.yaml` now resolve from the workspace root, including when scripts run in nested packages. Bare command names such as `bash` still use `PATH` [#14422](https://github.com/pnpm/pnpm/issues/14422).

- Fixed installing the pnpm version pinned in `packageManager` when `nodeLinker` is `hoisted`. Managed Node.js, Deno, and Bun installations also work when the global config uses `nodeLinker: hoisted` [#14595](https://github.com/pnpm/pnpm/issues/14595).

- The JavaScript pnpm can again switch to a project's pinned pnpm version on platforms without a native binary for that version, such as Alpine Linux with pnpm 10 or Intel Macs with pnpm 11. If a native pnpm version does not support the platform, the error now names the missing target [#13622](https://github.com/pnpm/pnpm/issues/13622).

- Provisioning Yarn 6 now uses `GH_TOKEN` or `GITHUB_TOKEN` when available to avoid GitHub's anonymous API rate limit in CI. Tokens are only sent when `strict-ssl` is enabled.

- Fixed concurrent installs sharing a global virtual store on macOS failing with "failed to import ... No such file or directory" [#14560](https://github.com/pnpm/pnpm/issues/14560).

- Fixed `pnpm setup` failing with `ERR_PNPM_DIRECTORY_FETCHER_PATH_ESCAPE` on Windows. Local `file:` dependencies whose directories are symlinks or junctions are now packed correctly [#14618](https://github.com/pnpm/pnpm/issues/14618).

- On Windows, installs now retry replacing command shims temporarily locked by another process [#14549](https://github.com/pnpm/pnpm/issues/14549).

- Fixed argument forwarding on Windows with `shellEmulator` enabled. Paths ending in a backslash, line breaks, and literal shell expressions are preserved [#14548](https://github.com/pnpm/pnpm/issues/14548).

- Windows store paths now consistently use backslashes in `pnpm store path` output and in the `storeDir` and `virtualStoreDir` fields of `node_modules/.modules.yaml`.

- Invalid certificates in `ca` or `cafile` no longer cause an `Invalid CA certificate` error. Valid certificates still apply, and blank `cert` or `key` values are treated as unset [#14646](https://github.com/pnpm/pnpm/issues/14646).

- Installs now respect the archive extraction concurrency limit even after a download is abandoned [#14585](https://github.com/pnpm/pnpm/issues/14585).

- `pnpm audit` summaries now exclude advisories ignored through `auditConfig.ignoreGhsas` and report them separately. When all advisories are ignored, the summary says so [#14535](https://github.com/pnpm/pnpm/issues/14535).

- `pnpm pack --json` now reports errors as JSON. Lifecycle script output appears before the final JSON output.

- `pnpm outdated -r` now wraps the `Dependents` column, keeping the table readable when many workspace projects use the same dependency [#14591](https://github.com/pnpm/pnpm/issues/14591).

- Shell completions now support the `pn` alias in bash, fish, pwsh, and zsh [#11955](https://github.com/pnpm/pnpm/issues/11955).

- `pnpm version` now accepts `-m` as a short alias for `--message` [#14567](https://github.com/pnpm/pnpm/issues/14567).
