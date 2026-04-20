# pnpm

## 11.0.0-rc.3

### Minor Changes

- Added a new `pnpm pack-app` command that packs a CommonJS entry file into a standalone executable for one or more target platforms, using the [Node.js Single Executable Applications](https://nodejs.org/api/single-executable-applications.html) API under the hood. Targets are specified as `<os>-<arch>[-<libc>]` (e.g. `linux-x64`, `linux-x64-musl`, `macos-arm64`, `win-x64`) and each produces an executable under `dist-app/<target>/` by default. Requires Node.js v25.5+ to perform the injection; an older host downloads Node.js v25 automatically.
- `pnpm audit --fix` now respects the `auditLevel` setting and supports a new interactive mode via `--interactive`/`-i`. Previously, `pnpm audit --fix` would fix all vulnerabilities regardless of the configured `auditLevel`, while `pnpm audit` (without `--fix`) correctly filtered by severity. Now both commands consistently filter advisories by the `auditLevel` setting, and you can use `pnpm audit --fix -i` to review and select which vulnerabilities to fix interactively.

  Overrides emitted by `pnpm audit --fix` now use a caret range (`^X.Y.Z`) instead of an open-ended `>=X.Y.Z`, so applying a security fix can no longer silently promote a dependency across a major version boundary.

- Added a new setting `minimumReleaseAgeIgnoreMissingTime`, which is `true` by default. When enabled, pnpm skips the `minimumReleaseAge` maturity check if the registry metadata does not include the `time` field. Set to `false` to fail resolution instead.
- Fixed and expanded `pnpm version` to match npm behavior:

  - Accept an explicit semver version (e.g. `pnpm version 1.2.3`) in addition to bump types.
  - Recognize `--no-commit-hooks`, `--no-git-tag-version`, `--sign-git-tag`, and `--message`.
  - Fix `--no-git-checks` which was previously parsed incorrectly.
  - Create a git commit and annotated tag for the version bump when running inside a git repository (unless `--no-git-tag-version` is used). `--message` supports `%s` replacement with the new version, and `--tag-version-prefix` controls the tag prefix (defaults to `v`). Git commits and tags are always skipped in recursive mode since multiple packages may be bumped to different versions in a single run [#11271](https://github.com/pnpm/pnpm/issues/11271).

- Renamed the platform-specific optional dependencies of `@pnpm/exe` to the new `@pnpm/exe.<platform>-<arch>[-<libc>]` scheme, using `process.platform` values (`linux`, `darwin`, `win32`) for the OS segment. The umbrella package `@pnpm/exe` itself is unchanged so existing `npm i -g @pnpm/exe` and `pnpm self-update` flows keep working.

  | before                    | after                        |
  | ------------------------- | ---------------------------- |
  | `@pnpm/linux-x64`         | `@pnpm/exe.linux-x64`        |
  | `@pnpm/linux-arm64`       | `@pnpm/exe.linux-arm64`      |
  | `@pnpm/linuxstatic-x64`   | `@pnpm/exe.linux-x64-musl`   |
  | `@pnpm/linuxstatic-arm64` | `@pnpm/exe.linux-arm64-musl` |
  | `@pnpm/macos-x64`         | `@pnpm/exe.darwin-x64`       |
  | `@pnpm/macos-arm64`       | `@pnpm/exe.darwin-arm64`     |
  | `@pnpm/win-x64`           | `@pnpm/exe.win32-x64`        |
  | `@pnpm/win-arm64`         | `@pnpm/exe.win32-arm64`      |

  GitHub release asset filenames follow the same scheme — `pnpm-linuxstatic-x64.tar.gz` becomes `pnpm-linux-x64-musl.tar.gz`, `pnpm-macos-*` becomes `pnpm-darwin-*`, `pnpm-win-*` becomes `pnpm-win32-*`. Anyone downloading releases directly needs to use the new filenames; `get.pnpm.io/install.sh` and `install.ps1` will be updated in lockstep to accept both schemes based on the requested version.

  Resolves [#11314](https://github.com/pnpm/pnpm/issues/11314).

### Patch Changes

- Do not print the `Cannot use both "packageManager" and "devEngines.packageManager" in package.json. "packageManager" will be ignored` warning when the two fields specify the exact same package manager name and version string. This lets projects keep both fields during the migration from `packageManager` to `devEngines.packageManager` without a noisy warning [#11301](https://github.com/pnpm/pnpm/issues/11301).
- Fix installing a directory dependency (`file:<dir>`) from an absolute path on a different drive on Windows. The directory fetcher was joining the stored directory onto `lockfileDir`, which on Windows concatenates an absolute cross-drive path literally (`path.join('D:\\...', 'C:\\Users\\...')` → `'D:\\...\\C:\\Users\\...'`). Use `path.resolve` so absolute paths are respected. This surfaced as an ENOENT during `pnpm setup` in CI when `PNPM_HOME` and the OS temp directory were on different drives.
- Fixed `pnpm sbom` and `pnpm licenses` failing to resolve license information for git-sourced dependencies (`git+https://`, `git+ssh://`, `github:` shorthand). These commands now correctly read the package manifest from the content-addressable store for `type: 'git'` resolutions [#11260](https://github.com/pnpm/pnpm/issues/11260).
- Fix `ERR_PNPM_OUTDATED_LOCKFILE` when approving builds during a global install. The `approve-builds` flow called by `pnpm add -g` passed the global packages directory to the subsequent install as `workspaceDir`, which caused sibling install directories (such as those left behind by `pnpm self-update`) to be picked up as workspace projects and fail the frozen-lockfile check.
- Restore the peer suffix encoding used by pnpm 10 for linked dependency paths. A `filenamify` upgrade changed how leading `./` and `../` segments were normalized, producing peer suffixes like `(b@+packages+b)` instead of `(b@packages+b)` for linked packages outside the workspace root, causing lockfile churn [#11272](https://github.com/pnpm/pnpm/issues/11272).
- Fix: different platform variants of the same runtime (e.g. `node@runtime:25.9.0` glibc vs. musl) no longer share a single global-virtual-store entry. The virtual store path now incorporates the selected variant's integrity, so installs with different `--os`/`--cpu`/`--libc` end up in separate directories and `pnpm add --libc=musl node@runtime:<v>` reliably fetches the musl binary even when the glibc variant is already cached.
- `pnpm sbom` now detects licenses declared via the deprecated `licenses` array in `package.json` (e.g. `busboy`, `streamsearch`, `limiter`) and falls back to scanning on-disk `LICENSE` files — mirroring the resolution logic of `pnpm licenses`. Previously these packages were reported as `NOASSERTION`. Shared license resolution (manifest parsing + LICENSE-file fallback) lives in the new `@pnpm/deps.compliance.license-resolver` package. When a manifest sets both `license` and `licenses`, the modern `license` field now takes precedence for both commands (previously `pnpm licenses` preferred `licenses`) [#11248](https://github.com/pnpm/pnpm/issues/11248).

## 11.0.0-rc.2

### Major Changes

- **Breaking:** removed the `managePackageManagerVersions`, `packageManagerStrict`, and `packageManagerStrictVersion` settings. They existed only to derive the `onFail` behavior for the legacy `packageManager` field, and the `pmOnFail` setting introduced alongside `pnpm with` subsumes all three — it directly sets the `onFail` behavior of both `packageManager` and `devEngines.packageManager`. The `COREPACK_ENABLE_STRICT` environment variable is no longer honored (it only gated `packageManagerStrict`); use `pmOnFail` instead.

  Migration:

  | Removed setting                       | Replace with                   |
  | ------------------------------------- | ------------------------------ |
  | `managePackageManagerVersions: true`  | `pmOnFail: download` (default) |
  | `managePackageManagerVersions: false` | `pmOnFail: ignore`             |
  | `packageManagerStrict: false`         | `pmOnFail: warn`               |
  | `packageManagerStrictVersion: true`   | `pmOnFail: error`              |
  | `COREPACK_ENABLE_STRICT=0`            | `pmOnFail: warn`               |

### Minor Changes

- `pnpm dlx` and `pnpm create` now respect security and trust policy settings (`minimumReleaseAge`, `minimumReleaseAgeExclude`, `minimumReleaseAgeStrict`, `trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`) from project-level configuration [#11183](https://github.com/pnpm/pnpm/issues/11183).
- Implemented native `star`, `unstar`, `stars`, and `whoami` commands.
- Add `pnpm with <version|current> <args...>` command. Runs pnpm at a specific version (or the currently active one) for a single invocation, bypassing the project's `packageManager` and `devEngines.packageManager` pins. Uses the same install mechanism as `pnpm self-update`, caching the downloaded pnpm in the global virtual store for reuse.

  Examples:

  ```
  pnpm with current install           # ignore the pinned version, use the running pnpm
  pnpm with 11.0.0-rc.1 install       # install using pnpm 11.0.0-rc.1
  pnpm with next install              # install using the "next" dist-tag
  ```

  Also adds a new `pmOnFail` setting that overrides the `onFail` behavior of `packageManager` and `devEngines.packageManager`. Accepted values: `download`, `error`, `warn`, `ignore`. Can be set via CLI flag, env var, `pnpm-workspace.yaml`, or `.npmrc` — useful when version management is handled by an external tool (asdf, mise, Volta, etc.) and the project wants pnpm itself to skip the check.

  ```
  pnpm install --pm-on-fail=ignore            # direct CLI flag
  pnpm_config_pm_on_fail=ignore pnpm install  # env var
  # or in pnpm-workspace.yaml:
  #   pmOnFail: ignore
  ```

- `pnpm init` now writes a `devEngines.packageManager` field instead of the `packageManager` field when `init-package-manager` is enabled.
- When pnpm is declared via the `packageManager` field in `package.json`, its resolution info is no longer written to `pnpm-lock.yaml` — unless the pinned pnpm version is v12 or newer. The `packageManagerDependencies` section is still populated (and reused across runs) when pnpm is declared via `devEngines.packageManager`. This makes the transition from pnpm v10 to v11 quieter by avoiding unnecessary lockfile churn for projects that pin an older pnpm in the legacy `packageManager` field.
- Added a new setting `runtimeOnFail` that overrides the `onFail` field of `devEngines.runtime` (and `engines.runtime`) in the root project's `package.json`. Accepted values: `ignore`, `warn`, `error`, `download`. For example, setting `runtimeOnFail=download` makes pnpm download the declared runtime version even when the manifest does not set `onFail: "download"`.

### Patch Changes

- `pnpm init` no longer adds the `devEngines.packageManager` field when run inside a workspace subpackage. The field is only added to the workspace root's `package.json`.

## 11.0.0-rc.1

### Major Changes

- `pnpm audit` now calls npm's `/-/npm/v1/security/advisories/bulk` endpoint. The legacy `/-/npm/v1/security/audits{,/quick}` endpoints have been retired by the registry, so the legacy request/response contract is no longer supported.

  The bulk endpoint does not return CVE identifiers. CVE-based filtering has been replaced with GitHub advisory ID (GHSA) filtering:

  - `auditConfig.ignoreCves` → `auditConfig.ignoreGhsas` (the previous key is no longer recognized)
  - `pnpm audit --ignore <id>` / `pnpm audit --ignore-unfixable` now read and write GHSAs instead of CVEs
  - GHSAs are derived from each advisory's `url` (`https://github.com/advisories/GHSA-xxxx-xxxx-xxxx`)

  To migrate: replace each `CVE-YYYY-NNNNN` entry in your `auditConfig.ignoreCves` with the corresponding `GHSA-xxxx-xxxx-xxxx` value (visible in the `More info` column of `pnpm audit` output) and move it under `auditConfig.ignoreGhsas`.

### Minor Changes

- Added the `pnpm docs` command and its alias `pnpm home`. This command opens the package documentation or homepage in the browser. When the package has no valid homepage, it falls back to `https://npmx.dev/package/<name>`.
- Added native `pnpm ping` command to test registry connectivity.
  Provides a simple way to verify connectivity to the configured registry without requiring external tools.
- Implemented native `search` command and its aliases (`s`, `se`, `find`).

### Patch Changes

- Fixed `pnpm store prune` removing packages used by the globally installed pnpm, breaking it.

## 11.0.0-rc.0

### Highlights

#### Major

- **Node.js 22+ required** — support for Node 18, 19, 20, and 21 is dropped, pnpm itself is now pure ESM, and the standalone exe requires glibc 2.27.
- **Supply-chain protection on by default** — `minimumReleaseAge` defaults to 1 day (newly published packages are not resolved for 24h) and `blockExoticSubdeps` defaults to `true`.
- **`allowBuilds` replaces the old build-dependency settings** — `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, `ignoredBuiltDependencies`, and `ignoreDepScripts` have been removed.
- **Global installs are isolated and use the global virtual store by default** — each `pnpm add -g` gets its own directory with its own `package.json`, `node_modules`, and lockfile.
- **New SQLite-backed store index** (store v11) with bundled manifests and hex digests, reducing filesystem syscalls and speeding up installation.
- **Native publish flow** — `pnpm publish`, `login`, `logout`, `view`, `deprecate`, `unpublish`, `dist-tag`, and `version` no longer delegate to the npm CLI, and the remaining npm passthrough commands now throw "not implemented".
- **`.npmrc` is auth/registry only** — all other settings must live in `pnpm-workspace.yaml` or the new global `config.yaml`, and environment variables use the `pnpm_config_*` prefix.

#### Minor

- **New commands:** `pnpm ci`, `pnpm sbom`, `pnpm clean`, `pnpm peers check`, and `pnpm runtime set`, plus `pn`/`pnx` short aliases.
- **ESM pnpmfiles** via `.pnpmfile.mjs`, which takes priority over `.pnpmfile.cjs` when present.
- **`pnpm audit --fix=update`** fixes vulnerabilities by updating packages in the lockfile instead of adding overrides.
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

- pnpm no longer falls back to the npm CLI. Commands that were previously passed through to npm (`access`, `bugs`, `docs`, `edit`, `find`, `home`, `issues`, `owner`, `ping`, `prefix`, `profile`, `pkg`, `repo`, `search`, `set-script`, `star`, `stars`, `team`, `token`, `unstar`, `whoami`, `xmas`) and their aliases (`s`, `se`) now throw a "not implemented" error, with a suggestion to use the npm CLI directly [#10642](https://github.com/pnpm/pnpm/pull/10642). Other previously passed-through commands — `view` (`info`, `show`, `v`), `login` (`adduser`), `logout`, `deprecate`, `unpublish`, `dist-tag`, and `version` — have been reimplemented natively in pnpm (see New Commands below).
- `pnpm publish` now works without the `npm` CLI.

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
- During install, instead of rendering the full peer dependency issues tree, pnpm now suggests running `pnpm peers check` to view the issues [#11133](https://github.com/pnpm/pnpm/pull/11133).

#### Lockfile

- Simplified `patchedDependencies` lockfile format from `Record<string, { path: string, hash: string }>` to `Record<string, string>` (selector to hash). Existing lockfiles with the old format are automatically migrated [#10911](https://github.com/pnpm/pnpm/pull/10911).

#### Other

- The default value of the `type` field in the `package.json` file of the project initialized by `pnpm init` command has been changed to `module`.
- Added support for lowercase options in `pnpm add`: `-d`, `-p`, `-o`, `-e` [#9197](https://github.com/pnpm/pnpm/issues/9197).

  When using the `pnpm add` command only:

  - `-p` is now an alias for `--save-prod` instead of `--parseable`
  - `-d` is now an alias for `--save-dev` instead of `--loglevel=info`

- The root workspace project is no longer excluded when it is explicitly selected via a filter [#10465](https://github.com/pnpm/pnpm/pull/10465).

### Minor Changes

#### New Commands

- Added native `pnpm view` (`info`, `show`, `v`) command for viewing package metadata from the registry [#11064](https://github.com/pnpm/pnpm/pull/11064).
- Added `pnpm login` (and `pnpm adduser` alias) command for authenticating with npm registries. Supports web-based login with QR code as well as classic username/password login [#11094](https://github.com/pnpm/pnpm/pull/11094).
- Added `pnpm logout` command for logging out of npm registries. Revokes the authentication token on the registry and removes it from the local auth config file [#11213](https://github.com/pnpm/pnpm/pull/11213).
- Added native `pnpm deprecate` and `pnpm undeprecate` commands for setting and removing deprecation messages on package versions without delegating to the npm CLI [#11120](https://github.com/pnpm/pnpm/pull/11120).
- Added native `pnpm unpublish` command. Supports unpublishing specific versions, version ranges via semver, and entire packages with `--force` [#11128](https://github.com/pnpm/pnpm/pull/11128).
- Added native `pnpm dist-tag` command (`ls`, `add`, `rm` subcommands) [#11218](https://github.com/pnpm/pnpm/pull/11218).
- Added `pnpm sbom` command for generating Software Bill of Materials in CycloneDX 1.7 and SPDX 2.3 JSON formats [#9088](https://github.com/pnpm/pnpm/issues/9088).
- Added `pnpm clean` command that safely removes `node_modules` directories from all workspace projects [#10707](https://github.com/pnpm/pnpm/issues/10707). Use `--lockfile` to also remove `pnpm-lock.yaml` files.
- Added a new command `pnpm runtime set <runtime name> <runtime version spec> [-g]` for installing runtimes. Deprecated `pnpm env use` in favor of the new command.
- Added the ability to fix vulnerabilities by updating packages in the lockfile instead of adding overrides. Use `pnpm audit --fix=update` [#10341](https://github.com/pnpm/pnpm/pull/10341).
- Added `pnpm ci` command for clean installs [#6100](https://github.com/pnpm/pnpm/issues/6100). The command runs `pnpm clean` followed by `pnpm install --frozen-lockfile`. Designed for CI/CD environments where reproducible builds are critical. Aliases: `pnpm clean-install`, `pnpm ic`, `pnpm install-clean` [#11003](https://github.com/pnpm/pnpm/pull/11003).
- Added `pnpm peers check` command that checks for unmet and missing peer dependency issues by reading the lockfile [#7087](https://github.com/pnpm/pnpm/issues/7087).
- Implemented the `version` command natively in pnpm to support workspaces and `workspace:` protocols correctly. The new command allows bumping package versions (major, minor, patch, etc.) with full workspace support and git integration [#10879](https://github.com/pnpm/pnpm/pull/10879).

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
