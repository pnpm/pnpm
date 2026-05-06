# pnpm

## 11.0.7

### Patch Changes

- Restore the execute bit on the `node-gyp` shims packed inside `@pnpm/exe` (`dist/node-gyp-bin/node-gyp`, `dist/node-gyp-bin/node-gyp.cmd`, and `dist/node_modules/node-gyp/bin/node-gyp.js`). Without this, `pnpm/action-setup`'s standalone path (used on runners with Node.js < 22.13) failed any install whose lifecycle script invoked `node-gyp rebuild` with `sh: 1: node-gyp: Permission denied` [#11483](https://github.com/pnpm/pnpm/issues/11483).
- Fixed the `pn`, `pnpx`, and `pnx` aliases failing in Git Bash / MSYS2 on Windows when pnpm was installed via `@pnpm/exe` (or after `pnpm self-update`) [#11486](https://github.com/pnpm/pnpm/issues/11486). Running `pnpx` (or `pnx`) printed the cmd.exe banner and dropped the user into an interactive command prompt instead of running `pnpm dlx`. The `bin` field rewrite on Windows was pointing those aliases at `.cmd` files; cmd-shim's Bash shim for a `.cmd` target wraps it in `exec cmd /C ...`, and MSYS2 mangles `/C` into a Windows path before cmd.exe sees it. The aliases are now `.exe` hardlinks of the SEA binary, which detects which name it was launched as via `process.execPath` and prepends `dlx` for `pnpx` / `pnx`.
- Fix `pnpm install` recreating `node_modules` after `pnpm fetch`. `pnpm fetch` records empty `hoistPattern` and `publicHoistPattern` in `.modules.yaml`; since v11 removed the explicit-config gate, the follow-up install treated those as a hoist-pattern change and purged the modules directory. The fetch step now flags the modules manifest with `virtualStoreOnly: true` so the next install skips the hoist-pattern comparison and completes the missing post-import linking in place [#11488](https://github.com/pnpm/pnpm/issues/11488).
- Pin the integrity of git-hosted tarballs (codeload.github.com, gitlab.com, bitbucket.org) in the lockfile so that subsequent installs detect a tampered or substituted tarball and refuse to install it. Previously the lockfile only stored the tarball URL for git dependencies, so a compromised git host or a man-in-the-middle could serve arbitrary code on later installs without lockfile changes.

  A new `gitHosted: true` field is recorded on git-hosted tarball resolutions in the lockfile, letting every reader/writer route them by a single typed check instead of pattern-matching the tarball URL in each call site. Lockfiles written by older pnpm versions are enriched on load (URL fallback) so the field can be relied on uniformly across the codebase.

- Allow user-level preferences in the global `config.yaml`. The following settings can now be set in `~/.config/pnpm/config.yaml` (or via `pnpm config set --location global`) instead of being restricted to `pnpm-workspace.yaml`: `agent`, `globalVirtualStoreDir`, `initPackageManager`, `initType`, `registrySupportsTimeField`, `scriptShell`, `shellEmulator`, `sideEffectsCache`, `sideEffectsCacheReadonly`, `stateDir`, `strictDepBuilds`, `trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`, `updateNotifier`, `useStderr`, `verifyDepsBeforeRun`, `verifyStoreIntegrity`, `virtualStoreDir`, `virtualStoreDirMaxLength` [#11474](https://github.com/pnpm/pnpm/issues/11474).
- Make trusted publishing (OIDC) take precedence over a configured static `_authToken` in `pnpm publish`, mirroring the npm CLI's behavior. When OIDC succeeds, the OIDC-derived token overrides any pre-configured `_authToken`; when OIDC is not applicable (no CI environment, exchange fails, registry has no trusted publisher configured), the static token is used as a fallback. This applies on every package during recursive publish, so each workspace package independently attempts trusted publishing.

  Additionally, the `NPM_ID_TOKEN` env var is now honored as a CI-agnostic injection point for an OIDC ID token. Previously OIDC was only attempted on GitHub Actions or GitLab; now any CI provider that exposes its own OIDC mechanism (e.g. CircleCI's `CIRCLE_OIDC_TOKEN_V2`, Buildkite, etc.) can forward its token via `NPM_ID_TOKEN` and trusted publishing will work without pnpm needing to recognize the provider explicitly.

- `--pm-on-fail=ignore` (and other universal options like `--loglevel`, `--reporter`) is now honored when combined with `--help` or `--version`. Previously the CLI argument parser short-circuited those flags before universal options were preserved, so `pnpm audit --pm-on-fail=ignore --help` and `pnpm --pm-on-fail=ignore --version` reported the strict packageManager mismatch instead of running the requested action [#11487](https://github.com/pnpm/pnpm/issues/11487).
- Fix a regression where `pnpm --recursive --filter '!<pkg>' run/exec/test/add` would include the workspace root in the matched projects. The workspace root is now correctly excluded by default when only negative `--filter` arguments are provided, matching the [documented behavior](https://pnpm.io/cli/recursive). To include the root, pass `--include-workspace-root` [#11341](https://github.com/pnpm/pnpm/issues/11341).
- Restore npm-CLI-compatible `--json` stdout output for `pnpm publish` ([#11476](https://github.com/pnpm/pnpm/issues/11476)). pnpm 11 reimplemented publish natively ([#10591](https://github.com/pnpm/pnpm/pull/10591)) and inadvertently dropped the per-package JSON object that pnpm 10 emitted transitively via the npm CLI, silently breaking downstream tooling — most notably `nx release publish`, which parses stdout JSON to confirm success ([nrwl/nx#35575](https://github.com/nrwl/nx/issues/35575)). On success, the output is now:

  - `pnpm publish --json` → single object `{ id, name, version, size, unpackedSize, shasum, integrity, filename, files, entryCount, bundled }`, mirroring `npm publish --json`.
  - `pnpm publish -r --json` → array of those objects, mirroring `pnpm pack --json`'s shape choice.
  - `pnpm publish -r --report-summary` → existing `pnpm-publish-summary.json` envelope `{ publishedPackages: [...] }` is preserved, but each entry is upgraded to the same per-package shape (additive — `name` and `version` are still present).

- `pnpm config get @<scope>:registry` now reports the same URL that `pnpm publish` and the resolvers actually use. Previously, `config get` only consulted `.npmrc`, while `publish`/install used the merged map that includes `pnpm-workspace.yaml`'s `registries` block — so the two could diverge silently and a publish could go to the wrong registry [#11492](https://github.com/pnpm/pnpm/issues/11492).

## 11.0.6

### Patch Changes

- Fix `pnpm_config_npmrc_auth_file` and `pnpm_config_userconfig` env vars not actually loading the custom `.npmrc`. The env vars were parsed and assigned to the resolved config, but only after `loadNpmrcConfig` had already read the default `~/.npmrc` — so the custom file path was set but never read. The relevant env vars are now consulted before the user-level `.npmrc` is loaded [#11465](https://github.com/pnpm/pnpm/issues/11465).
- Preserve the original key order in `pnpm-workspace.yaml` when updating it. Existing keys keep their position, and new keys are inserted in alphabetical position when the existing keys are already sorted (with a leading `packages` key allowed) or appended at the end otherwise.
- Fixed `pnpm self-update` on installations originally set up by pnpm v10. v10 added `PNPM_HOME` directly to PATH and wrote a `pnpm` bootstrap shim there. v11 setup writes shims under `PNPM_HOME/bin` instead, so when a v10 user upgrades to v11 the legacy shim at `PNPM_HOME` keeps pointing into the old `.tools/<version>` install — `pnpm --version` continues to report the pre-update version even though the new version was installed under `global/v11`. Self-update now detects this layout, refreshes the legacy shims so the upgrade actually takes effect, and prints a hint suggesting `pnpm setup` to migrate PATH to the v11 layout. [#11464](https://github.com/pnpm/pnpm/issues/11464).
- Print a warning when settings that are not allowed in the global config file (e.g. `nodeLinker`, `hoistPattern`) are present in `config.yaml` and silently ignored. Previously these settings were dropped without any feedback, leaving users unsure why their global configuration had no effect. The warning suggests moving those settings to a project-level `pnpm-workspace.yaml`, or sharing them across projects via [config dependencies](https://pnpm.io/11.x/config-dependencies).
- Throw a pnpm error when `overrides` has an invalid shape or contains a non-string value.
- Validate all `readPackage` dependency map fields, including `devDependencies`, and reject falsy non-object invalid values instead of silently accepting them.
- Prevent crashes during `pnpm config`, `pnpm set`, and `pnpm get` by tolerating `configDependencies` install failures. For these commands, a failure to install `configDependencies` (for example because the registry auth token has not been written yet) is now logged at debug level and the command proceeds. All other commands still surface the install error [#10684](https://github.com/pnpm/pnpm/issues/10684).
- Treat `allowBuilds` as an install-state input and clear previously ignored builds when they are explicitly disallowed.
- Fixes #10594, catalogs not being read from the workspace when using the `catalog:` protocol with the `pnpm dlx` / `pnpx` command, resulting in a catalog entry not found error.
- Accept `PNPM_CONFIG_*` (uppercase) environment variables in addition to `pnpm_config_*`. Previously, only the lowercase form was honored, so env vars renamed per the v11 migration guide (e.g. `PNPM_CONFIG_USERCONFIG`) silently had no effect on case-sensitive systems like macOS and Linux [#11465](https://github.com/pnpm/pnpm/issues/11465).

## 11.0.5

### Patch Changes

- Drop the `darwin-x64` artifact from `@pnpm/exe` and from the GitHub release page. The Node.js SEA mechanism `pnpm pack-app` uses produces a binary that segfaults at startup on Intel Macs because of an upstream Node.js bug ([nodejs/node#62893](https://github.com/nodejs/node/issues/62893), tracked alongside [#59553](https://github.com/nodejs/node/issues/59553); the Node.js team has [opted not to fix it](https://github.com/nodejs/node/pull/60250) on the grounds that x64 macOS is being phased out). Re-signing with `codesign` or `ldid` doesn't help — the corruption is in LIEF's Mach-O surgery, before signing.

  Intel Mac users should install pnpm via `npm install -g pnpm` (uses the system Node.js, no SEA), or stay on pnpm 10.x. `@pnpm/exe`'s preinstall on Intel Mac now exits with a clear error pointing at these alternatives.

  Closes [#11423](https://github.com/pnpm/pnpm/issues/11423).

- `pnpm dlx` (and `pnpx`/`pnx`/`pnpm create`) now runs the same interactive `approve-builds` prompt as `pnpm add -g` when the package being launched depends on transitive packages with install scripts. Previously, the v11 `strictDepBuilds` default made dlx fail with `ERR_PNPM_IGNORED_BUILDS` and required users to re-run with `--allow-build=<pkg>` for every offending dependency. dlx also now removes the partially-populated cache directory when the install fails, so a subsequent run starts clean instead of reusing a broken install whose builds were silently skipped [#11444](https://github.com/pnpm/pnpm/issues/11444).
- 72629fc: Fix `pnpm -g ls --json` and `pnpm -g ls --parseable` so they emit valid JSON and parseable output respectively, matching pnpm 10 behavior. Since the isolated global packages refactor in pnpm 11, the global list command had a custom path that always printed plain text and ignored `--json`/`--parseable`, which broke tools like `npm-check-updates` that parse the JSON output [#11440](https://github.com/pnpm/pnpm/issues/11440).

  `pnpm -g ls --depth=<n>` (with n > 0) now errors when more than one isolated global install would be involved, since each install has its own lockfile and merging their transitive trees would be incoherent. When the request can be narrowed to a single install group, the regular `list` flow is used and the full dependency tree is shown.

- Fixed `pnpm publish` to honor `publishConfig.registry` from `package.json` when publishing a single package. The native publish flow introduced in v11 was reading the registry from `.npmrc` only, ignoring the per-package override [#11419](https://github.com/pnpm/pnpm/issues/11419).
- When `strictPeerDependencies` is `true`, the `ERR_PNPM_PEER_DEP_ISSUES` error once again renders the peer dependency issues inline using the same format as `pnpm peers check`, so users (and CI tools like Renovate) can see what failed without running `pnpm peers check` separately [#11439](https://github.com/pnpm/pnpm/issues/11439).
- The `WARN` and error code labels in pnpm's output now wrap in brackets (`[WARN]`, `[ERR_PNPM_FOO]`). Previously the labels relied entirely on a colored background to stand out, which meant they blended into the surrounding text in terminals without color (e.g. when `NO_COLOR` is set or output is piped). The brackets are painted in the same color as the badge background, so they appear as ordinary padding in color-capable terminals — only the no-color rendering changes.

## 11.0.4

### Patch Changes

- Fixed `pnpm ci` not reinstalling workspace package `node_modules` directories after the clean step [#11427](https://github.com/pnpm/pnpm/issues/11427).
- Remove pnpm's workspace state file when cleaning node_modules so `pnpm ci` performs a fresh install after the clean step.
- Do not remove `pnpm-lock.yaml` during `pnpm clean` when `lockfile: true` is configured in `pnpm-workspace.yaml`. The lockfile is only removed when the `--lockfile` option is passed to `pnpm clean`.
- `pnpm self-update` (with no version argument) no longer downgrades pnpm when the registry's `latest` dist-tag points to an older release than the currently active version. Run `pnpm self-update latest` to force a downgrade [#11418](https://github.com/pnpm/pnpm/issues/11418).
- `minimumReleaseAgeStrict` now defaults to `true` whenever the user explicitly sets `minimumReleaseAge` (via `pnpm-workspace.yaml`, the global `config.yaml`, the CLI, or `pnpm_config_*` env vars).

## 11.0.3

### Patch Changes

- Fix too many open files error sometimes happening on Windows, when creating command shims in `node_modules/.bin` [#11412](https://github.com/pnpm/pnpm/issues/11412).
- Fix `ERR_PNPM_FETCH_404` when installing a project whose lockfile depends on a `file:` tarball. The previous behavior dropped the `tarball` field from `file:` and git-hosted resolutions when `lockfile-include-tarball-url=false` (the default), even though those URLs cannot be reconstructed from the package name, version, and registry [#11407](https://github.com/pnpm/pnpm/issues/11407).

## 11.0.2

### Patch Changes

- Fix `ENOENT` symlink failure when `pnpm add -g` triggers the approve-builds prompt. The global add flow used to forward an absolute `modulesDir` (`<installDir>/node_modules`) into the install run by `approve-builds`. The install layer treated `modulesDir` as a path relative to `lockfileDir` and joined it again, producing a doubled path on Windows because `path.join` does not collapse an embedded absolute path. The hoist step then tried to `mkdir` and symlink under `<installDir>\<installDir>\node_modules\.pnpm\node_modules\...` and failed with `ENOENT` [#11403](https://github.com/pnpm/pnpm/issues/11403).
- Fixed `packageManagerDependencies` going stale when pnpm is invoked through corepack. The lockfile sync (and the `devEngines.packageManager` version check) previously ran only when pnpm was invoked directly; under corepack the entire block was skipped, so a stale entry would persist even after the running pnpm version changed. The lockfile sync now runs regardless of how pnpm was invoked, while the pnpm-managed version switch (`onFail: 'download'`) remains skipped under corepack so it doesn't fight corepack's own version selection [#11397](https://github.com/pnpm/pnpm/issues/11397).
- Fix recursive publish summaries to report the manifest from `publishConfig.directory` when packages publish from a generated directory [#11239](https://github.com/pnpm/pnpm/issues/11239).
- Fix negated `os` / `cpu` entries (e.g. `["!win32"]`) being incorrectly rejected when `supportedArchitectures` expands to multiple platforms [#11375](https://github.com/pnpm/pnpm/pull/11375).

## 11.0.1

### Patch Changes

- Report unknown top-level options before falling back to implicit `pnpm run` scripts.
- Reject `null` named catalogs in workspace manifests with `InvalidWorkspaceManifestError` instead of crashing with a raw `TypeError`.
- Populate download location for git-sourced dependencies in SBOM output. Previously `pnpm sbom` emitted `NOASSERTION` (SPDX) and omitted the distribution reference (CycloneDX) for git dependencies. Now emits the git URL with commit hash, e.g. `git+https://github.com/user/repo.git#commit`.
- `pnpm self-update` now keeps `package.json`'s `packageManager` and `devEngines.packageManager` in sync. When the legacy `packageManager` field pins pnpm, both fields are rewritten to the new exact pnpm version on update — `packageManager` to `pnpm@<version>` (without an integrity hash), and `devEngines.packageManager.version` to the same exact `<version>` (dropping any range operator). When only `devEngines.packageManager` is declared, the existing range-preserving behavior is unchanged [#11388](https://github.com/pnpm/pnpm/issues/11388).
- Sort the keys of the overrides object returned by `pnpm audit --fix` so that the log output order matches the order written to `pnpm-workspace.yaml`.
- Update the env lockfile's `packageManagerDependencies` entry when `devEngines.packageManager` declares a pnpm version that the lockfile no longer satisfies. Previously, the stale entry was kept even though the running pnpm matched the declared version, silently breaking the integrity record [#11387](https://github.com/pnpm/pnpm/issues/11387).

## 11.0.0

### Highlights

#### Major

- **Node.js 22+ required** — support for Node 18, 19, 20, and 21 is dropped, pnpm itself is now pure ESM, and the standalone exe requires glibc 2.27.
- **Supply-chain protection on by default** — `minimumReleaseAge` defaults to 1 day (newly published packages are not resolved for 24h) and `blockExoticSubdeps` defaults to `true`.
- **`allowBuilds` replaces the old build-dependency settings** — `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, `ignoredBuiltDependencies`, and `ignoreDepScripts` have been removed.
- **Global installs are isolated and use the global virtual store by default** — each `pnpm add -g` gets its own directory with its own `package.json`, `node_modules`, and lockfile.
- **New SQLite-backed store index** (store v11) with bundled manifests and hex digests, reducing filesystem syscalls and speeding up installation.
- **Native publish flow** — [`pnpm publish`](https://pnpm.io/11.x/cli/publish), [`login`](https://pnpm.io/11.x/cli/login), [`logout`](https://pnpm.io/11.x/cli/logout), [`view`](https://pnpm.io/11.x/cli/view), [`deprecate`](https://pnpm.io/11.x/cli/deprecate), [`unpublish`](https://pnpm.io/11.x/cli/unpublish), [`dist-tag`](https://pnpm.io/11.x/cli/dist-tag), and [`version`](https://pnpm.io/11.x/cli/version) no longer delegate to the npm CLI, and the remaining npm passthrough commands now throw "not implemented".
- **[`pnpm audit`](https://pnpm.io/11.x/cli/audit) uses npm's bulk advisories endpoint** — the legacy `/security/audits` endpoints are gone. CVE-based filtering has been replaced with GHSA-based filtering: migrate `auditConfig.ignoreCves` entries to `auditConfig.ignoreGhsas`.
- **`.npmrc` is auth/registry only** — all other settings must live in `pnpm-workspace.yaml` or the new global `config.yaml`, and environment variables use the `pnpm_config_*` prefix.
- **Runtime installs are slimmer** — installing a Node.js runtime via `node@runtime:<version>` no longer extracts the bundled `npm`, `npx`, and `corepack`, roughly halving the files pnpm has to hash, write, and link.

#### Minor

- **New commands:** [`pnpm ci`](https://pnpm.io/11.x/cli/ci), [`pnpm sbom`](https://pnpm.io/11.x/cli/sbom), [`pnpm clean`](https://pnpm.io/11.x/cli/clean), [`pnpm peers check`](https://pnpm.io/11.x/cli/peers), [`pnpm runtime set`](https://pnpm.io/11.x/cli/runtime), [`pnpm docs`](https://pnpm.io/11.x/cli/docs)/`home`, [`pnpm ping`](https://pnpm.io/11.x/cli/ping), [`pnpm search`](https://pnpm.io/11.x/cli/search), [`pnpm star`](https://pnpm.io/11.x/cli/star)/`unstar`/`stars`, [`pnpm whoami`](https://pnpm.io/11.x/cli/whoami), [`pnpm with`](https://pnpm.io/11.x/cli/with), and [`pnpm pack-app`](https://pnpm.io/11.x/cli/pack-app), plus `pn`/[`pnx`](https://pnpm.io/11.x/cli/pnx) short aliases.
- **ESM pnpmfiles** via `.pnpmfile.mjs`, which takes priority over `.pnpmfile.cjs` when present.
- **[`pnpm audit --fix=update`](https://pnpm.io/11.x/cli/audit)** fixes vulnerabilities by updating packages in the lockfile instead of adding overrides, and `pnpm audit --fix --interactive` lets you select which advisories to fix.
- **[`pnpm pack-app`](https://pnpm.io/11.x/cli/pack-app)** packs a CommonJS entry into a standalone executable for one or more target platforms using Node.js Single Executable Applications.
- **Faster HTTP and I/O** — undici with Happy Eyeballs, direct-to-CAS writes, skipped staging directory, pre-allocated tarball downloads, and an NDJSON metadata cache.

### Major Changes

#### Requirements

- pnpm is now distributed as pure ESM.
- Dropped support for Node.js v18, 19, 20, and 21.
- The standalone exe version of pnpm requires at least glibc 2.27.

#### Security & Build Defaults

- Changed default values: `optimisticRepeatInstall` is now `true`, `verifyDepsBeforeRun` is now `install`, `minimumReleaseAge` is now `1440` (1 day), and `minimumReleaseAgeStrict` is `false`. Newly published packages will not be resolved until they are at least 1 day old. This protects against supply chain attacks by giving the community time to detect and remove compromised versions. To opt out, set `minimumReleaseAge: 0` in `pnpm-workspace.yaml` [#11158](https://github.com/pnpm/pnpm/pull/11158).
- `strictDepBuilds` is `true` by default.
- `blockExoticSubdeps` is `true` by default.
- Removed deprecated build dependency settings: `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, `ignoredBuiltDependencies`, and `ignoreDepScripts` [#11220](https://github.com/pnpm/pnpm/pull/11220).

  Use the `allowBuilds` setting instead. It is a map where keys are package name patterns and values are booleans:

  - `true` means the package is allowed to run build scripts
  - `false` means the package is explicitly denied from running build scripts

  Same as before, by default, none of the packages in the dependencies are allowed to run scripts. If a package has postinstall scripts and it isn't declared in `allowBuilds`, an error is printed.

  Before:

  ```yaml
  onlyBuiltDependencies:
    - electron
  onlyBuiltDependenciesFile: "allowed-builds.json"
  neverBuiltDependencies:
    - core-js
  ignoredBuiltDependencies:
    - esbuild
  ```

  After:

  ```yaml
  allowBuilds:
    electron: true
    core-js: false
    esbuild: false
  ```

- Removed `allowNonAppliedPatches` in favor of `allowUnusedPatches`.
- Removed `ignorePatchFailures`; patch application failures now throw an error.

#### Store

- Runtime dependencies are always linked from the global virtual store [#10233](https://github.com/pnpm/pnpm/pull/10233).
- Optimized index file format to store the hash algorithm once per file instead of repeating it for every file entry. Each file entry now stores only the hex digest instead of the full integrity string (`<algo>-<digest>`). Using hex format improves performance since file paths in the content-addressable store use hex representation, eliminating base64-to-hex conversion during path lookups.
- Store version bumped to v11.
- The bundled manifest (name, version, bin, engines, scripts, etc.) is now stored directly in the package index file, eliminating the need to read `package.json` from the content-addressable store during resolution and installation. This reduces I/O and speeds up repeat installs [#10473](https://github.com/pnpm/pnpm/pull/10473).
- The package index in the content-addressable store is now backed by SQLite. Instead of individual JSON files under `$STORE/index/`, package metadata is stored in a single SQLite database at `$STORE/index.db` with MessagePack-encoded values. This reduces filesystem syscall overhead, improves space efficiency for small metadata entries, and enables concurrent access via SQLite's WAL mode. Packages missing from the new index are re-fetched on demand [#10500](https://github.com/pnpm/pnpm/pull/10500) [#10826](https://github.com/pnpm/pnpm/issues/10826).

#### Global Packages

- Global installs (`pnpm add -g pkg`) and `pnx` now use the global virtual store by default. Packages are stored at `{storeDir}/links` instead of per-project `.pnpm` directories. This can be disabled by setting `enableGlobalVirtualStore: false` [#10694](https://github.com/pnpm/pnpm/pull/10694).
- Isolated global packages. Each globally installed package (or group of packages installed together) now gets its own isolated installation directory with its own `package.json`, `node_modules/`, and lockfile. This prevents global packages from interfering with each other through peer dependency conflicts, hoisting changes, or version resolution shifts.

  Key changes:

  - `pnpm add -g <pkg>` creates an isolated installation in `{pnpmHomeDir}/global/v11/{hash}/`
  - `pnpm remove -g <pkg>` removes the entire installation group containing the package
  - `pnpm update -g [pkg]` re-installs packages in new isolated directories
  - `pnpm list -g` scans isolated directories to show all installed global packages
  - `pnpm install -g` (no args) is no longer supported; use `pnpm add -g <pkg>` instead

- Globally installed binaries are now stored in a `bin` subdirectory of `PNPM_HOME` instead of directly in `PNPM_HOME`. This prevents internal directories like `global/` and `store/` from polluting shell autocompletion when `PNPM_HOME` is on PATH [#10986](https://github.com/pnpm/pnpm/issues/10986). After upgrading, run `pnpm setup` to update your shell configuration.
- Breaking changes to `pnpm link`:

  - `pnpm link <pkg-name>` no longer resolves packages from the global store. Only relative or absolute paths are accepted. For example, use `pnpm link ./foo` instead of `pnpm link foo`.
  - `pnpm link --global` is removed. Use `pnpm add -g .` to register a local package's bins globally.
  - `pnpm link` (no arguments) is removed. Use `pnpm link <dir>` with an explicit path instead.

#### Configuration

- pnpm no longer reads all settings from `.npmrc`. Only auth and registry settings are read from `.npmrc` files. All other settings (like `hoistPattern`, `nodeLinker`, `shamefullyHoist`, etc.) must be configured in `pnpm-workspace.yaml` or the global `~/.config/pnpm/config.yaml` [#11189](https://github.com/pnpm/pnpm/pull/11189).
- Network settings (`httpProxy`, `httpsProxy`, `noProxy`, `localAddress`, `strictSsl`, `gitShallowHosts`) are now written to `config.yaml` (global) or `pnpm-workspace.yaml` (local) instead of `.npmrc`/`auth.ini`. They are still readable from `.npmrc` for easier migration from the npm CLI [#11209](https://github.com/pnpm/pnpm/pull/11209).

  pnpm no longer reads `npm_config_*` environment variables. Use `pnpm_config_*` environment variables instead (e.g., `pnpm_config_registry` instead of `npm_config_registry`).

  pnpm no longer reads the npm global config at `$PREFIX/etc/npmrc`.

  `pnpm login` writes auth tokens to `~/.config/pnpm/auth.ini`.

  New `registries` setting in `pnpm-workspace.yaml`:

  ```yaml
  registries:
    default: https://registry.npmjs.org/
    "@my-org": https://private.example.com/
    "@internal": https://nexus.corp.com/
  ```

  Auth tokens in `~/.npmrc` still work — pnpm continues to read `~/.npmrc` as a fallback for registry authentication. The new `npmrcAuthFile` setting can be used to point to a different file instead of `~/.npmrc`.

- Replace workspace project specific `.npmrc` with `packageConfigs` in `pnpm-workspace.yaml`.

  A workspace manifest with `packageConfigs` looks something like this:

  ```yaml
  # File: pnpm-workspace.yaml
  packages:
    - "packages/project-1"
    - "packages/project-2"
  packageConfigs:
    "project-1":
      saveExact: true
    "project-2":
      savePrefix: "~"
  ```

  Or this:

  ```yaml
  # File: pnpm-workspace.yaml
  packages:
    - "packages/project-1"
    - "packages/project-2"
  packageConfigs:
    - match: ["project-1", "project-2"]
      modulesDir: "node_modules"
      saveExact: true
  ```

- pnpm no longer reads settings from the `pnpm` field of `package.json`. Settings should be defined in `pnpm-workspace.yaml` [#10086](https://github.com/pnpm/pnpm/pull/10086).
- `pnpm config get` (without `--json`) no longer prints INI formatted text. Instead, it prints JSON for objects and arrays, and raw strings for strings, numbers, booleans, and nulls. `pnpm config get --json` still prints all types of values as JSON, as before.
- `pnpm config get <array>` now prints a JSON array.
- `pnpm config list` now prints a JSON object instead of INI formatted text.
- `pnpm config list` and `pnpm config get` (without argument) now hide auth-related settings.
- `pnpm config list` and `pnpm config get` (without argument) now show top-level keys as camelCase. Exception: keys that start with `@` or `//` are preserved (their cases don't change).
- `pnpm config get` and `pnpm config list` no longer load non-camelCase options from the workspace manifest (`pnpm-workspace.yaml`).

#### Removed Commands & npm Passthrough

- pnpm no longer falls back to the npm CLI. Commands that were previously passed through to npm (`access`, `bugs`, `docs`, `edit`, `find`, `home`, `issues`, `owner`, `ping`, `prefix`, `profile`, `pkg`, `repo`, `search`, `set-script`, `star`, `stars`, `team`, `token`, `unstar`, `whoami`, `xmas`) and their aliases (`s`, `se`) now throw a "not implemented" error, with a suggestion to use the npm CLI directly [#10642](https://github.com/pnpm/pnpm/pull/10642). Other previously passed-through commands — [`view`](https://pnpm.io/11.x/cli/view) (`info`, `show`, `v`), [`login`](https://pnpm.io/11.x/cli/login) (`adduser`), [`logout`](https://pnpm.io/11.x/cli/logout), [`deprecate`](https://pnpm.io/11.x/cli/deprecate), [`unpublish`](https://pnpm.io/11.x/cli/unpublish), [`dist-tag`](https://pnpm.io/11.x/cli/dist-tag), and [`version`](https://pnpm.io/11.x/cli/version) — have been reimplemented natively in pnpm (see New Commands below).
- [`pnpm publish`](https://pnpm.io/11.x/cli/publish) now works without the `npm` CLI.

  The One-time Password feature now reads from `PNPM_CONFIG_OTP` instead of `NPM_CONFIG_OTP`:

  ```sh
  export PNPM_CONFIG_OTP='<your OTP here>'
  pnpm publish --no-git-checks
  ```

  If the registry requests OTP and the user has not provided it via the `PNPM_CONFIG_OTP` environment variable or the `--otp` flag, pnpm will prompt the user directly for an OTP code.

  If the registry requests web-based authentication, pnpm will print a scannable QR code along with the URL.

  Since the new `pnpm publish` no longer calls `npm publish`, some undocumented features may have been unknowingly dropped. If you rely on a feature that is now gone, please open an issue at <https://github.com/pnpm/pnpm/issues>. In the meantime, you can use `pnpm pack && npm publish *.tgz` as a workaround.

- Removed the `pnpm server` command [#10463](https://github.com/pnpm/pnpm/pull/10463).
- Removed support for the `useNodeVersion` and `executionEnv.nodeVersion` fields. `devEngines.runtime` and `engines.runtime` should be used instead [#10373](https://github.com/pnpm/pnpm/pull/10373).
- Removed support for `hooks.fetchers`. We now have a new API for custom fetchers and resolvers via the `fetchers` field of `pnpmfile`.

#### Lifecycle Scripts

- pnpm no longer populates `npm_config_*` environment variables from the pnpm config during lifecycle scripts. Only well-known `npm_*` env vars are now set, matching Yarn's behavior [#11116](https://github.com/pnpm/pnpm/pull/11116).

#### CLI Output

- Cleaner output for script execution: pnpm now prints `$ command` instead of `> pkg@version stage path\n> command`, and shows project name and path only when running in a different directory. The `$ command` line is printed to stderr to keep stdout clean for piping [#11132](https://github.com/pnpm/pnpm/pull/11132).
- During install, instead of rendering the full peer dependency issues tree, pnpm now suggests running [`pnpm peers check`](https://pnpm.io/11.x/cli/peers) to view the issues [#11133](https://github.com/pnpm/pnpm/pull/11133).

#### Lockfile

- Simplified `patchedDependencies` lockfile format from `Record<string, { path: string, hash: string }>` to `Record<string, string>` (selector to hash). Existing lockfiles with the old format are automatically migrated [#10911](https://github.com/pnpm/pnpm/pull/10911).

#### Other

- The default value of the `type` field in the `package.json` file of the project initialized by `pnpm init` command has been changed to `module`.
- Added support for lowercase options in `pnpm add`: `-d`, `-p`, `-o`, `-e` [#9197](https://github.com/pnpm/pnpm/issues/9197).

  When using the `pnpm add` command only:

  - `-p` is now an alias for `--save-prod` instead of `--parseable`
  - `-d` is now an alias for `--save-dev` instead of `--loglevel=info`

- The root workspace project is no longer excluded when it is explicitly selected via a filter [#10465](https://github.com/pnpm/pnpm/pull/10465).

#### Audit

- [`pnpm audit`](https://pnpm.io/11.x/cli/audit) now calls npm's `/-/npm/v1/security/advisories/bulk` endpoint. The legacy `/-/npm/v1/security/audits{,/quick}` endpoints have been retired by the registry, so the legacy request/response contract is no longer supported.

  The bulk endpoint does not return CVE identifiers. CVE-based filtering has been replaced with GitHub advisory ID (GHSA) filtering:

  - `auditConfig.ignoreCves` → `auditConfig.ignoreGhsas` (the previous key is no longer recognized)
  - `pnpm audit --ignore <id>` / `pnpm audit --ignore-unfixable` now read and write GHSAs instead of CVEs
  - GHSAs are derived from each advisory's `url` (`https://github.com/advisories/GHSA-xxxx-xxxx-xxxx`)

  To migrate: replace each `CVE-YYYY-NNNNN` entry in your `auditConfig.ignoreCves` with the corresponding `GHSA-xxxx-xxxx-xxxx` value (visible in the `More info` column of `pnpm audit` output) and move it under `auditConfig.ignoreGhsas`.

#### Package Manager Settings

- **Breaking:** removed the `managePackageManagerVersions`, `packageManagerStrict`, and `packageManagerStrictVersion` settings. They existed only to derive the `onFail` behavior for the legacy `packageManager` field, and the `pmOnFail` setting introduced alongside [`pnpm with`](https://pnpm.io/11.x/cli/with) subsumes all three — it directly sets the `onFail` behavior of both `packageManager` and `devEngines.packageManager`. The `COREPACK_ENABLE_STRICT` environment variable is no longer honored (it only gated `packageManagerStrict`); use `pmOnFail` instead.

  Migration:

  | Removed setting                       | Replace with                   |
  | ------------------------------------- | ------------------------------ |
  | `managePackageManagerVersions: true`  | `pmOnFail: download` (default) |
  | `managePackageManagerVersions: false` | `pmOnFail: ignore`             |
  | `packageManagerStrict: false`         | `pmOnFail: warn`               |
  | `packageManagerStrictVersion: true`   | `pmOnFail: error`              |
  | `COREPACK_ENABLE_STRICT=0`            | `pmOnFail: warn`               |

#### Runtime Installs

- Installing a Node.js runtime via `node@runtime:<version>` (including `pnpm env use` and `pnpm runtime set node`) no longer extracts the bundled `npm`, `npx`, and `corepack` from the Node.js archive. This cuts roughly half of the files pnpm has to hash, write to the CAS, and link during installation, making runtime installs noticeably faster. Users who still need `npm` can install it as a separate package.

### Minor Changes

#### New Commands

- Added native [`pnpm view`](https://pnpm.io/11.x/cli/view) (`info`, `show`, `v`) command for viewing package metadata from the registry [#11064](https://github.com/pnpm/pnpm/pull/11064).
- Added [`pnpm login`](https://pnpm.io/11.x/cli/login) (and `pnpm adduser` alias) command for authenticating with npm registries. Supports web-based login with QR code as well as classic username/password login [#11094](https://github.com/pnpm/pnpm/pull/11094).
- Added [`pnpm logout`](https://pnpm.io/11.x/cli/logout) command for logging out of npm registries. Revokes the authentication token on the registry and removes it from the local auth config file [#11213](https://github.com/pnpm/pnpm/pull/11213).
- Added native [`pnpm deprecate`](https://pnpm.io/11.x/cli/deprecate) and `pnpm undeprecate` commands for setting and removing deprecation messages on package versions without delegating to the npm CLI [#11120](https://github.com/pnpm/pnpm/pull/11120).
- Added native [`pnpm unpublish`](https://pnpm.io/11.x/cli/unpublish) command. Supports unpublishing specific versions, version ranges via semver, and entire packages with `--force` [#11128](https://github.com/pnpm/pnpm/pull/11128).
- Added native [`pnpm dist-tag`](https://pnpm.io/11.x/cli/dist-tag) command (`ls`, `add`, `rm` subcommands) [#11218](https://github.com/pnpm/pnpm/pull/11218).
- Added [`pnpm sbom`](https://pnpm.io/11.x/cli/sbom) command for generating Software Bill of Materials in CycloneDX 1.7 and SPDX 2.3 JSON formats [#9088](https://github.com/pnpm/pnpm/issues/9088).
- Added [`pnpm clean`](https://pnpm.io/11.x/cli/clean) command that safely removes `node_modules` directories from all workspace projects [#10707](https://github.com/pnpm/pnpm/issues/10707). Use `--lockfile` to also remove `pnpm-lock.yaml` files.
- Added a new command [`pnpm runtime set <runtime name> <runtime version spec> [-g]`](https://pnpm.io/11.x/cli/runtime) for installing runtimes. Deprecated `pnpm env use` in favor of the new command.
- Added the ability to fix vulnerabilities by updating packages in the lockfile instead of adding overrides. Use [`pnpm audit --fix=update`](https://pnpm.io/11.x/cli/audit) [#10341](https://github.com/pnpm/pnpm/pull/10341).
- Added [`pnpm ci`](https://pnpm.io/11.x/cli/ci) command for clean installs [#6100](https://github.com/pnpm/pnpm/issues/6100). The command runs `pnpm clean` followed by `pnpm install --frozen-lockfile`. Designed for CI/CD environments where reproducible builds are critical. Aliases: `pnpm clean-install`, `pnpm ic`, `pnpm install-clean` [#11003](https://github.com/pnpm/pnpm/pull/11003).
- Added [`pnpm peers check`](https://pnpm.io/11.x/cli/peers) command that checks for unmet and missing peer dependency issues by reading the lockfile [#7087](https://github.com/pnpm/pnpm/issues/7087).
- Implemented the [`version`](https://pnpm.io/11.x/cli/version) command natively in pnpm to support workspaces and `workspace:` protocols correctly. The new command allows bumping package versions (major, minor, patch, etc.) with full workspace support and git integration [#10879](https://github.com/pnpm/pnpm/pull/10879).
- [`pnpm audit --fix`](https://pnpm.io/11.x/cli/audit) now supports a new interactive mode via `--interactive`/`-i`.
- Added the [`pnpm docs`](https://pnpm.io/11.x/cli/docs) command and its alias `pnpm home`. This command opens the package documentation or homepage in the browser. When the package has no valid homepage, it falls back to `https://npmx.dev/package/<name>`.
- Added native [`pnpm ping`](https://pnpm.io/11.x/cli/ping) command to test registry connectivity. Provides a simple way to verify connectivity to the configured registry without requiring external tools.
- Implemented native [`search`](https://pnpm.io/11.x/cli/search) command and its aliases (`s`, `se`, `find`).
- Implemented native [`star`, `unstar`, `stars`](https://pnpm.io/11.x/cli/star), and [`whoami`](https://pnpm.io/11.x/cli/whoami) commands.
- Add [`pnpm with <version|current> <args...>`](https://pnpm.io/11.x/cli/with) command. Runs pnpm at a specific version (or the currently active one) for a single invocation, bypassing the project's `packageManager` and `devEngines.packageManager` pins.
- Added a new [`pnpm pack-app`](https://pnpm.io/11.x/cli/pack-app) command that packs a CommonJS entry file into a standalone executable for one or more target platforms, using the [Node.js Single Executable Applications](https://nodejs.org/api/single-executable-applications.html) API under the hood.

#### Configuration

- Added support for a global YAML config file named `config.yaml`.

  Configuration is now split into two categories:

  - Registry and auth settings, which can be stored in INI files such as the global `rc` file and local `.npmrc`.
  - pnpm-specific settings, which can only be loaded from YAML files such as the global `config.yaml` and local `pnpm-workspace.yaml`.

- Added support for loading environment variables whose names start with `pnpm_config_` into config. These environment variables override settings from `pnpm-workspace.yaml` but not CLI arguments.
- Added support for reading `allowBuilds` from `pnpm-workspace.yaml` in the global package directory for global installs.
- Added support for `pnpm config get globalconfig` to retrieve the global config file path [#9977](https://github.com/pnpm/pnpm/issues/9977).
- Added a new setting `virtualStoreOnly` that populates the virtual store without creating importer symlinks, hoisting, bin links, or running lifecycle scripts. This is useful for pre-populating a store (e.g., in Nix builds) without creating unnecessary project-level artifacts. `pnpm fetch` now uses this mode internally [#10840](https://github.com/pnpm/pnpm/issues/10840).
- Added support for specifying the pnpm version via `devEngines.packageManager` in `package.json`. Unlike the `packageManager` field, this supports version ranges. The resolved version is stored in `pnpm-lock.yaml` and reused if it still satisfies the range [#10932](https://github.com/pnpm/pnpm/pull/10932).
- Added a new `dedupePeers` setting that reduces peer dependency duplication. When enabled, peer dependency suffixes use version-only identifiers (`name@version`) instead of full dep paths, eliminating nested suffixes like `(foo@1.0.0(bar@2.0.0))`. This dramatically reduces the number of package instances in projects with many recursive peer dependencies [#11070](https://github.com/pnpm/pnpm/issues/11070).
- Config dependencies are now installed into the global virtual store (`{storeDir}/links/`) and symlinked into `node_modules/.pnpm-config/`. This allows config dependencies to be shared across projects that use the same store, avoiding redundant fetches and imports [#10910](https://github.com/pnpm/pnpm/pull/10910). Config dependency and package manager integrity info is now stored in `pnpm-lock.yaml` instead of inlined in `pnpm-workspace.yaml`: the workspace manifest contains only clean version specifiers for `configDependencies`, while the resolved versions, integrity hashes, and tarball URLs are recorded in the lockfile as a separate YAML document. The env lockfile section also stores `packageManagerDependencies` resolved during version switching and self-update. Projects using the old inline-hash format are automatically migrated on install [#10912](https://github.com/pnpm/pnpm/pull/10912) [#10964](https://github.com/pnpm/pnpm/pull/10964).
- Added `nodeDownloadMirrors` setting to configure custom Node.js download mirrors in `pnpm-workspace.yaml`. This replaces the `node-mirror:<channel>` `.npmrc` setting, which is no longer read [#11194](https://github.com/pnpm/pnpm/pull/11194):

  ```yaml
  nodeDownloadMirrors:
    release: https://my-mirror.example.com/download/release/
  ```

- `pnpm dlx` and `pnpm create` now respect security and trust policy settings (`minimumReleaseAge`, `minimumReleaseAgeExclude`, `minimumReleaseAgeStrict`, `trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`) from project-level configuration [#11183](https://github.com/pnpm/pnpm/issues/11183).
- `pnpm init` now writes a `devEngines.packageManager` field instead of the `packageManager` field when `init-package-manager` is enabled.
- Added a new setting `runtimeOnFail` that overrides the `onFail` field of `devEngines.runtime` (and `engines.runtime`) in the root project's `package.json`. Accepted values: `ignore`, `warn`, `error`, `download`. For example, setting `runtimeOnFail=download` makes pnpm download the declared runtime version even when the manifest does not set `onFail: "download"`.
- Added a new setting `minimumReleaseAgeIgnoreMissingTime`, which is `true` by default. When enabled, pnpm skips the `minimumReleaseAge` maturity check if the registry metadata does not include the `time` field. Set to `false` to fail resolution instead.

#### Store

- When the global virtual store is enabled, packages that are not allowed to build (and don't transitively depend on packages that are) now get hashes that don't include the engine name (platform, architecture, Node.js major version). This means ~95% of packages in the GVS survive Node.js upgrades and architecture changes without re-import [#10837](https://github.com/pnpm/pnpm/issues/10837).

#### Hooks & Pnpmfiles

- Added support for pnpmfiles written in ESM, using the `.mjs` extension. When `.pnpmfile.mjs` exists, it takes priority over `.pnpmfile.cjs` and only one is loaded [#9730](https://github.com/pnpm/pnpm/pull/9730).

#### CLI & Other

- The built-in `clean`, `setup`, `deploy`, and `rebuild` commands now prefer user scripts over built-in commands. When a project's `package.json` has a script with the same name, `pnpm` executes the script instead of the built-in command. Added `purge` as an alias for the built-in `clean` command, which always runs the built-in regardless of scripts [#11118](https://github.com/pnpm/pnpm/pull/11118).
- Added `-F` as a short alias for the `--filter` option.
- Added support for hidden scripts. Scripts starting with `.` are hidden and cannot be run directly via `pnpm run`. They can only be called from other scripts. Hidden scripts are also omitted from the `pnpm run` listing [#11041](https://github.com/pnpm/pnpm/pull/11041).
- `pnpm approve-builds` now accepts positional arguments for approving or denying packages without the interactive prompt. Prefix a package name with `!` to deny it. Only mentioned packages are affected; the rest are left untouched [#11030](https://github.com/pnpm/pnpm/pull/11030).
- During install, packages with ignored builds that are not yet listed in `allowBuilds` are automatically added to `pnpm-workspace.yaml` with a placeholder value, so users can manually set them to `true` or `false` [#11030](https://github.com/pnpm/pnpm/pull/11030).
- Added `pn` and `pnx` short aliases for `pnpm` and `pnpx` (`pnpm dlx`) [#11052](https://github.com/pnpm/pnpm/pull/11052).
- `pnpm store prune` now displays the total size of removed files [#11047](https://github.com/pnpm/pnpm/pull/11047).
- `pnpm audit --fix` now adds the minimum patched version for each advisory to `minimumReleaseAgeExclude` in `pnpm-workspace.yaml`, so the security fix can be installed without waiting for `minimumReleaseAge` [#11216](https://github.com/pnpm/pnpm/pull/11216).
- pnpm now warns when `optimisticRepeatInstall` skips `shouldRefreshResolution` hooks [#10995](https://github.com/pnpm/pnpm/pull/10995).

#### Performance

- Replaced `node-fetch` with native `undici` for HTTP requests throughout pnpm [#10537](https://github.com/pnpm/pnpm/pull/10537).
- Eliminated redundant internal linking during GVS warm reinstall when no packages were added [#11073](https://github.com/pnpm/pnpm/pull/11073).
- Eliminated the staging directory when importing packages into `node_modules`, avoiding the overhead of creating a temp dir and renaming per package [#11088](https://github.com/pnpm/pnpm/pull/11088).
- CAS files are now written directly to their final content-addressed path instead of to a temp file and renamed. This eliminates ~30k rename syscalls per cold install [#11087](https://github.com/pnpm/pnpm/pull/11087).
- Optimized hot-path string operations in the content-addressable store and increased `gunzipSync` chunk size for fewer buffer allocations during tarball decompression [#11086](https://github.com/pnpm/pnpm/pull/11086).
- Improved HTTP performance with Happy Eyeballs (dual-stack), better keep-alive settings, and an optimized global dispatcher. Tarball downloads with known size now pre-allocate memory to avoid double-copy overhead [#11151](https://github.com/pnpm/pnpm/pull/11151).
- Adopted `If-Modified-Since` for conditional metadata fetches, avoiding re-downloading unchanged registry metadata [#11161](https://github.com/pnpm/pnpm/pull/11161).
- Switched to abbreviated metadata when checking `minimumReleaseAge`, reducing the amount of data fetched from the registry [#11160](https://github.com/pnpm/pnpm/pull/11160).
- Switched the metadata cache to NDJSON format, improving read/write performance [#11188](https://github.com/pnpm/pnpm/pull/11188).

### Patch Changes

- Switched to `process.stderr.write` instead of `console.error` for script logging [#11140](https://github.com/pnpm/pnpm/pull/11140).
- Respected the `frozen-lockfile` flag when migrating config dependencies [#11067](https://github.com/pnpm/pnpm/pull/11067).
- Removed the `--workspace` flag from the `version` command [#11115](https://github.com/pnpm/pnpm/pull/11115).
- Handled `ENOTSUP` error in the clone import path during parallel I/O [#11117](https://github.com/pnpm/pnpm/pull/11117).
- Fixed `pnpm audit` command.
- Updated dependencies to fix vulnerabilities.
- pnpm now checks whether a package is installable for non-npm-hosted packages (e.g., git or tarball dependencies) after the manifest has been fetched.
- pnpm now explicitly passes the path of the global `rc` config file to `npm`.
- Fixed YAML formatting preservation in `pnpm-workspace.yaml` when running commands like `pnpm update`. Previously, quotes and other formatting were lost even when catalog values didn't change.

  Closes #10425

- The parameter set by the `--allow-build` flag is now written to `allowBuilds`.
- Fixed a bug in which specifying `filter` in `pnpm-workspace.yaml` would cause pnpm to not detect any projects.
- Deferred patch errors until all patches in a group are applied, so that one failed patch does not prevent other patches from being attempted.
- pnpm now fails on incompatible lockfiles in CI when frozen lockfile mode is enabled [#10978](https://github.com/pnpm/pnpm/pull/10978).
- Fixed `strictDepBuilds` and `allowBuilds` checks being bypassed when a package's build side-effects are cached in the store [#11039](https://github.com/pnpm/pnpm/pull/11039).
- In GVS mode, `pnpm approve-builds` now runs a full install instead of rebuild, ensuring that GVS hash directories and symlinks are updated correctly after changing `allowBuilds` [#11043](https://github.com/pnpm/pnpm/pull/11043).
- Fixed a crash in the lockfile merger when merging non-semver version strings (e.g. `link:`, `file:`, git URLs) [#11102](https://github.com/pnpm/pnpm/pull/11102).
- Handled `ENOTSUP` error in `linkOrCopy` during parallel imports [#11103](https://github.com/pnpm/pnpm/pull/11103).
- Skipped linking bins that already reference the correct target. This avoids redundant I/O during repeated installs and prevents permission errors when the store is read-only (e.g. Docker layer caching, CI prewarm, NFS) [#11069](https://github.com/pnpm/pnpm/pull/11069).
- Fixed `_password` handling for the default registry to decode from base64 before use, consistent with scoped registry behavior [#11089](https://github.com/pnpm/pnpm/pull/11089).
- Fixed a bug where the CAS locker cache was not updated when a file already existed with correct integrity [#11085](https://github.com/pnpm/pnpm/pull/11085).
- Prevented catalog entries from being removed by `cleanupUnusedCatalogs` when they are referenced only from workspace `overrides` [#11075](https://github.com/pnpm/pnpm/pull/11075).
- Resolved patch file paths during `pnpm fetch` [#11054](https://github.com/pnpm/pnpm/pull/11054).
- Fixed invalid specifiers for peers on all non-exact version selectors [#11049](https://github.com/pnpm/pnpm/pull/11049).
- Fixed false "Command not found" error on Windows when the command exists but exits with a non-zero exit code [#11000](https://github.com/pnpm/pnpm/issues/11000).
- Prepended `Bearer` to the authorization token generated by `tokenHelper` if it is missing, aligning with npm's behavior [#11097](https://github.com/pnpm/pnpm/pull/11097).
- Propagated error cause when throwing `PnpmError` in `@pnpm/npm-resolver` [#10990](https://github.com/pnpm/pnpm/pull/10990).
- Fixed SQLite race condition during store initialization on Windows.
- Removed `rimrafSync` in `importIndexedDir` fast-path error handler [#11168](https://github.com/pnpm/pnpm/pull/11168).
- Fixed `pnpm dedupe --check` unexpectedly failing due to non-deterministic resolution [#11110](https://github.com/pnpm/pnpm/pull/11110).
- Fixed empty files not being rejected in `isEmptyDirOrNothing` [#11182](https://github.com/pnpm/pnpm/pull/11182).
- Fixed `.bat`/`.cmd` token helpers not working on Windows due to missing `shell: true` option.
