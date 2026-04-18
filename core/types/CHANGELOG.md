# @pnpm/types

## 1101.0.0

### Major Changes

- ff28085: `pnpm audit` now calls npm's `/-/npm/v1/security/advisories/bulk` endpoint. The legacy `/-/npm/v1/security/audits{,/quick}` endpoints have been retired by the registry, so the legacy request/response contract is no longer supported.

  The bulk endpoint does not return CVE identifiers. CVE-based filtering has been replaced with GitHub advisory ID (GHSA) filtering:

  - `auditConfig.ignoreCves` → `auditConfig.ignoreGhsas` (the previous key is no longer recognized)
  - `pnpm audit --ignore <id>` / `pnpm audit --ignore-unfixable` now read and write GHSAs instead of CVEs
  - GHSAs are derived from each advisory's `url` (`https://github.com/advisories/GHSA-xxxx-xxxx-xxxx`)

  To migrate: replace each `CVE-YYYY-NNNNN` entry in your `auditConfig.ignoreCves` with the corresponding `GHSA-xxxx-xxxx-xxxx` value (visible in the `More info` column of `pnpm audit` output) and move it under `auditConfig.ignoreGhsas`.

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.
- efb48dc: DevEngineDependency renamed to EngineDependency.
- cb367b9: Remove deprecated build dependency settings: `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies`.
- 7b1c189: Removed the deprecated `allowNonAppliedPatches` completely in favor of `allowUnusedPatches`.
  Remove `ignorePatchFailures` so all patch application failures should throw an error.
- 71de2b3: Removed support for the `useNodeVersion` and `executionEnv.nodeVersion` fields. `devEngines.runtime` and `engines.runtime` should be used instead [#10373](https://github.com/pnpm/pnpm/pull/10373).

### Minor Changes

- 76718b3: Added support for `allowBuilds`, which is a new field that can be used instead of `onlyBuiltDependencies` and `ignoredBuiltDependencies`. The new `allowBuilds` field in your `pnpm-workspace.yaml` uses a map of package matchers to explicitly allow (`true`) or disallow (`false`) script execution. This allows for a single, easy-to-manage source of truth for your build permissions.

  **Example Usage.** To explicitly allow all versions of `esbuild` to run scripts and prevent `core-js` from running them:

  ```yaml
  allowBuilds:
    esbuild: true
    core-js: false
  ```

  The example above achieves the same result as the previous configuration:

  ```yaml
  onlyBuiltDependencies:
    - esbuild
  ignoredBuiltDependencies:
    - core-js
  ```

  Related PR: [#10311](https://github.com/pnpm/pnpm/pull/10311)

- cc1b8e3: Fixed installation of config dependencies from private registries.

  Added support for object type in `configDependencies` when the tarball URL returned from package metadata differs from the computed URL [#10431](https://github.com/pnpm/pnpm/pull/10431).

- 05fb1ae: Add type for IgnoredBuilds.
- 10bc391: Added a new setting: `trustPolicy`.
- 2df8b71: pnpm no longer reads all settings from `.npmrc`. Only auth and registry settings are read from `.npmrc` files. All other settings (like `hoist-pattern`, `node-linker`, `shamefully-hoist`, etc.) must be configured in `pnpm-workspace.yaml` or the global `~/.config/pnpm/config.yaml`.

  ### What changed

  **`.npmrc` is now only for auth and registry settings.** pnpm-specific settings in `.npmrc` are ignored. Move them to `pnpm-workspace.yaml`.

  **pnpm no longer reads `npm_config_*` environment variables.** Use `pnpm_config_*` environment variables instead (e.g., `pnpm_config_registry` instead of `npm_config_registry`).

  **pnpm no longer reads the npm global config** at `$PREFIX/etc/npmrc`.

  **`pnpm login` writes auth tokens** to `~/.config/pnpm/auth.ini`.

  ### Settings still read from `.npmrc`

  The following settings continue to be read from `.npmrc` files (project-level and `~/.npmrc`):

  - `registry` and `@scope:registry` — registry URLs
  - `//registry.example.com/:_authToken` — auth tokens per registry
  - `_auth`, `_authToken`, `_password`, `username`, `email` — global auth credentials
  - `//registry.example.com/:tokenHelper` — token helper commands
  - `ca`, `cafile`, `cert`, `key`, `certfile`, `keyfile` — SSL certificates
  - `strict-ssl` — SSL verification
  - `proxy`, `https-proxy`, `no-proxy` — proxy settings
  - `local-address` — local network address binding
  - `git-shallow-hosts` — git shallow clone hosts

  ### New `npmrcAuthFile` setting

  A new `npmrcAuthFile` setting can be added to `pnpm-workspace.yaml` or `~/.config/pnpm/config.yaml` to specify a custom path to the user `.npmrc` file (defaults to `~/.npmrc`):

  ```yaml
  npmrcAuthFile: /custom/path/.npmrc
  ```

  ### New `registries` setting in `pnpm-workspace.yaml`

  Registry URLs can now be configured in `pnpm-workspace.yaml`, so there's no need to commit `.npmrc` files with registry mappings:

  ```yaml
  registries:
    default: https://registry.npmjs.org/
    "@my-org": https://private.example.com/
    "@internal": https://nexus.corp.com/
  ```

  This replaces the `.npmrc` settings `registry=...` and `@scope:registry=...`.

  ### Auth file read order (highest priority first)

  1. `~/.config/pnpm/auth.ini` — pnpm's own auth file (written by `pnpm login`)
  2. `<workspace>/.npmrc` — workspace root (or project root)
  3. `~/.npmrc` (or custom `npmrcAuthFile`) — user-level fallback

  Note: `.npmrc` is only read from the workspace root, not from individual package directories.

  ### Migration guide

  1. **Move pnpm settings from `.npmrc` to `pnpm-workspace.yaml`:**

     Before (`.npmrc`):

     ```ini
     shamefully-hoist=true
     node-linker=hoisted
     ```

     After (`pnpm-workspace.yaml`):

     ```yaml
     shamefullyHoist: true
     nodeLinker: hoisted
     ```

  2. **Move scoped registry mappings from `.npmrc` to `pnpm-workspace.yaml`:**

     Before (`.npmrc`):

     ```ini
     @my-org:registry=https://private.example.com
     ```

     After (`pnpm-workspace.yaml`):

     ```yaml
     registries:
       "@my-org": https://private.example.com/
     ```

  3. **If you use `npm_config_*` env vars**, switch to `pnpm_config_*`:

     ```sh
     # Before
     npm_config_registry=https://registry.example.com

     # After
     pnpm_config_registry=https://registry.example.com
     ```

  4. **Auth tokens in `~/.npmrc` still work.** No migration needed for registry authentication — pnpm continues to read `~/.npmrc` as a fallback.

- 15549a9: Add the ability to fix vulnerabilities by updating packages in the lockfile instead of adding overrides.
- cc7c0d2: `pnpm publish` now works without the `npm` CLI.

  The One-time Password feature now reads from `PNPM_CONFIG_OTP` instead of `NPM_CONFIG_OTP`:

  ```sh
  export PNPM_CONFIG_OTP='<your OTP here>'
  pnpm publish --no-git-checks
  ```

  If the registry requests OTP and the user has not provided it via the `PNPM_CONFIG_OTP` environment variable or the `--otp` flag, pnpm will prompt the user directly for an OTP code.

  If the registry requests web-based authentication, pnpm will print a scannable QR code along with the URL.

  Since the new `pnpm publish` no longer calls `npm publish`, some undocumented features may have been unknowingly dropped. If you rely on a feature that is now gone, please open an issue at <https://github.com/pnpm/pnpm/issues>. In the meantime, you can use `pnpm pack && npm publish *.tgz` as a workaround.

- efb48dc: **Node.js Runtime Installation for Dependencies.** Added support for automatic Node.js runtime installation for dependencies. pnpm will now install the Node.js version required by a dependency if that dependency declares a Node.js runtime in the "engines" field. For example:

  ```json
  {
    "engines": {
      "runtime": {
        "name": "node",
        "version": "^24.11.0",
        "onFail": "download"
      }
    }
  }
  ```

  If the package with the Node.js runtime dependency is a CLI app, pnpm will bind the CLI app to the required Node.js version. This ensures that, regardless of the globally installed Node.js instance, the CLI will use the compatible version of Node.js.

  If the package has a `postinstall` script, that script will be executed using the specified Node.js version.

  Related PR: [#10141](https://github.com/pnpm/pnpm/pull/10141)

### Patch Changes

- a8f016c: Store config dependency and package manager integrity info in `pnpm-lock.yaml` instead of inlining it in `pnpm-workspace.yaml`. The workspace manifest now contains only clean version specifiers for `configDependencies`, while the resolved versions, integrity hashes, and tarball URLs are recorded in the lockfile as a separate YAML document. The env lockfile section also stores `packageManagerDependencies` resolved during version switching and self-update. Projects using the old inline-hash format are automatically migrated on install.
- 8ffb1a7: `pnpm list` and `pnpm why` now display npm: protocol for aliased packages (e.g., `foo npm:is-odd@3.0.1`) [#8660](https://github.com/pnpm/pnpm/issues/8660).

## 1000.9.0

### Minor Changes

- 7c1382f: Add `PackageVersionPolicy` function type.
- dee39ec: You can now allow specific versions of dependencies to run postinstall scripts. `onlyBuiltDependencies` now accepts package names with lists of trusted versions. For example:

  ```yaml
  onlyBuiltDependencies:
    - nx@21.6.4 || 21.6.5
    - esbuild@0.25.1
  ```

  Related PR: [#10104](https://github.com/pnpm/pnpm/pull/10104).

## 1000.8.0

### Minor Changes

- e792927: Added support for `finders` [#9946](https://github.com/pnpm/pnpm/pull/9946).

## 1000.7.0

### Minor Changes

- 1a07b8f: Added "devEngines" to the manifest fields.

## 1000.6.0

### Minor Changes

- 5ec7255: Export AuditConfig.

## 1000.5.0

### Minor Changes

- 5b73df1: Added `PinnedVersion`.

## 1000.4.0

### Minor Changes

- 750ae7d: Export `ConfigDependencies` type.

## 1000.3.0

### Minor Changes

- 5f7be64: Rename `pnpm.allowNonAppliedPatches` to `pnpm.allowUnusedPatches`. The old name is still supported but it would print a deprecation warning message.
- 5f7be64: Add `pnpm.ignorePatchFailures` to manage whether pnpm would ignore patch application failures.

  If `ignorePatchFailures` is not set, pnpm would throw an error when patches with exact versions or version ranges fail to apply, and it would ignore failures from name-only patches.

  If `ignorePatchFailures` is explicitly set to `false`, pnpm would throw an error when any type of patch fails to apply.

  If `ignorePatchFailures` is explicitly set to `true`, pnpm would print a warning when any type of patch fails to apply.

## 1000.2.1

### Patch Changes

- a5e4965: Fix `pnpm deploy` creating a `package.json` without the `imports` and `license` field [#9193](https://github.com/pnpm/pnpm/issues/9193).

## 1000.2.0

### Minor Changes

- 8fcc221: Export PnpmSettings.

## 1000.1.1

### Patch Changes

- b562deb: Fix `pnpm deploy` creating a package.json without the `"type"` key [#8962](https://github.com/pnpm/pnpm/issues/8962).

## 1000.1.0

### Minor Changes

- 9591a18: Added support for a new type of dependencies called "configurational dependencies". These dependencies are installed before all the other types of dependencies (before "dependencies", "devDependencies", "optionalDependencies").

  Configurational dependencies cannot have dependencies of their own or lifecycle scripts. They should be added using exact version and the integrity checksum. Example:

  ```json
  {
    "pnpm": {
      "configDependencies": {
        "my-configs": "1.0.0+sha512-30iZtAPgz+LTIYoeivqYo853f02jBYSd5uGnGpkFV0M3xOt9aN73erkgYAmZU43x4VfqcnLxW9Kpg3R5LC4YYw=="
      }
    }
  }
  ```

  Related RFC: [#8](https://github.com/pnpm/rfcs/pull/8).
  Related PR: [#8915](https://github.com/pnpm/pnpm/pull/8915).

## 12.2.0

### Minor Changes

- d500d9f: Added a new setting to `package.json` at `pnpm.auditConfig.ignoreGhsas` for ignoring vulnerabilities by their GHSA code [#6838](https://github.com/pnpm/pnpm/issues/6838).

  For instance:

  ```json
  {
    "pnpm": {
      "auditConfig": {
        "ignoreGhsas": [
          "GHSA-42xw-2xvc-qx8m",
          "GHSA-4w2v-q235-vp99",
          "GHSA-cph5-m8f7-6c5x",
          "GHSA-vh95-rmgr-6w4m"
        ]
      }
    }
  }
  ```

## 12.1.0

### Minor Changes

- 7ee59a1: Added optional modulesDir field to projects.

## 12.0.0

### Major Changes

- cb006df: Add ability to apply patch to all versions:
  If the key of `pnpm.patchedDependencies` is a package name without a version (e.g. `pkg`), pnpm will attempt to apply the patch to all versions of
  the package, failure will be skipped.
  If it is a package name and an exact version (e.g. `pkg@x.y.z`), pnpm will attempt to apply the patch to that exact version only, failure will
  cause pnpm to fail.

  If there's only one version of `pkg` installed, `pnpm patch pkg` and subsequent `pnpm patch-commit $edit_dir` will create an entry named `pkg` in
  `pnpm.patchedDependencies`. And pnpm will attempt to apply this patch to other versions of `pkg` in the future.

  If there's multiple versions of `pkg` installed, `pnpm patch pkg` will ask which version to edit and whether to attempt to apply the patch to all.
  If the user chooses to apply the patch to all, `pnpm patch-commit $edit_dir` would create a `pkg` entry in `pnpm.patchedDependencies`.
  If the user chooses not to apply the patch to all, `pnpm patch-commit $edit_dir` would create a `pkg@x.y.z` entry in `pnpm.patchedDependencies` with
  `x.y.z` being the version the user chose to edit.

  If the user runs `pnpm patch pkg@x.y.z` with `x.y.z` being the exact version of `pkg` that has been installed, `pnpm patch-commit $edit_dir` will always
  create a `pkg@x.y.z` entry in `pnpm.patchedDependencies`.

## 11.1.0

### Minor Changes

- 0ef168b: Support specifying node version (via `pnpm.executionEnv.nodeVersion` in `package.json`) for running lifecycle scripts per each package in a workspace [#6720](https://github.com/pnpm/pnpm/issues/6720).

## 11.0.0

### Major Changes

- dd00eeb: Renamed dir to rootDir in the Project object.
- Breaking change to the graph type

## 10.1.1

### Patch Changes

- 13e55b2: If install is performed on a subset of workspace projects, always create an up-to-date lockfile first. So, a partial install can be performed only on a fully resolved (non-partial) lockfile [#8165](https://github.com/pnpm/pnpm/issues/8165).

## 10.1.0

### Minor Changes

- 45f4262: Add PkgResolutionId.

## 10.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- 7733f3a: Added support for registry-scoped SSL configurations (cert, key, and ca). Three new settings supported: `<registryURL>:certfile`, `<registryURL>:keyfile`, and `<registryURL>:ca`. For instance:

  ```
  //registry.mycomp.com/:certfile=server-cert.pem
  //registry.mycomp.com/:keyfile=server-key.pem
  //registry.mycomp.com/:cafile=client-cert.pem
  ```

  Related issue: [#7427](https://github.com/pnpm/pnpm/issues/7427).
  Related PR: [#7626](https://github.com/pnpm/pnpm/pull/7626).

- 730929e: Add a field named `ignoredOptionalDependencies`. This is an array of strings. If an optional dependency has its name included in this array, it will be skipped.

## 9.4.2

### Patch Changes

- 4d34684f1: Added support for boolean values in 'bundleDependencies' package.json fields when installing a dependency. Fix to properly handle 'bundledDependencies' alias [#7411](https://github.com/pnpm/pnpm/issues/7411).

## 9.4.1

### Patch Changes

- Added support for boolean values in 'bundleDependencies' package.json fields when installing a dependency. Fix to properly handle 'bundledDependencies' alias [#7411](https://github.com/pnpm/pnpm/issues/7411).

## 9.4.0

### Minor Changes

- 43ce9e4a6: Support for multiple architectures when installing dependencies [#5965](https://github.com/pnpm/pnpm/issues/5965).

  You can now specify architectures for which you'd like to install optional dependencies, even if they don't match the architecture of the system running the install. Use the `supportedArchitectures` field in `package.json` to define your preferences.

  For example, the following configuration tells pnpm to install optional dependencies for Windows x64:

  ```json
  {
    "pnpm": {
      "supportedArchitectures": {
        "os": ["win32"],
        "cpu": ["x64"]
      }
    }
  }
  ```

  Whereas this configuration will have pnpm install optional dependencies for Windows, macOS, and the architecture of the system currently running the install. It includes artifacts for both x64 and arm64 CPUs:

  ```json
  {
    "pnpm": {
      "supportedArchitectures": {
        "os": ["win32", "darwin", "current"],
        "cpu": ["x64", "arm64"]
      }
    }
  }
  ```

  Additionally, `supportedArchitectures` also supports specifying the `libc` of the system.

## 9.3.0

### Minor Changes

- d774a3196: The list of packages that are allowed to run installation scripts now may be provided in a separate configuration file. The path to the file should be specified via the `pnpm.onlyBuiltDependenciesFile` field in `package.json`. For instance:

  ```json
  {
    "dependencies": {
      "@my-org/policy": "1.0.0"
    }
    "pnpm": {
      "onlyBuiltDependenciesFile": "node_modules/@my-org/policy/allow-build.json"
    }
  }
  ```

  In the example above, the list is loaded from a dependency. The JSON file with the list should contain an array of package names. For instance:

  ```json
  ["esbuild", "@reflink/reflink"]
  ```

  With the above list, only `esbuild` and `@reflink/reflink` will be allowed to run scripts during installation.

  Related issue: [#7137](https://github.com/pnpm/pnpm/issues/7137).

## 9.2.0

### Minor Changes

- aa2ae8fe2: Support `publishConfig.registry` in `package.json` for publishing [#6775](https://github.com/pnpm/pnpm/issues/6775).

## 9.1.0

### Minor Changes

- a9e0b7cbf: Add new types.

## 9.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

## 8.10.0

### Minor Changes

- b77651d14: New setting supported in the `package.json` that is in the root of the workspace: `pnpm.requiredScripts`. Scripts listed in this array will be required in each project of the workspace. Otherwise, `pnpm -r run <script name>` will fail [#5569](https://github.com/pnpm/pnpm/issues/5569).

## 8.9.0

### Minor Changes

- 702e847c1: A new setting supported for ignoring vulnerabilities by their CVEs. The ignored CVEs may be listed in the `pnpm.auditConfig.ignoreCves` field of `package.json`. For instance:

  ```
  {
    "pnpm": {
      "auditConfig": {
        "ignoreCves": [
          "CVE-2019-10742",
          "CVE-2020-28168",
          "CVE-2021-3749",
          "CVE-2020-7598"
        ]
      }
    }
  }
  ```

## 8.8.0

### Minor Changes

- 844e82f3a: New type exported: DependenciesOrPeersField

## 8.7.0

### Minor Changes

- d665f3ff7: Ignore packages listed in package.json > pnpm.updateConfig.ignoreDependencies fields on update/outdated command [#5358](https://github.com/pnpm/pnpm/issues/5358)

## 8.6.0

### Minor Changes

- 156cc1ef6: A new setting supported in the pnpm section of the `package.json` file: `allowNonAppliedPatches`. When it is set to `true`, non-applied patches will not cause an error, just a warning will be printed. For example:

  ```json
  {
    "name": "foo",
    "version": "1.0.0",
    "pnpm": {
      "patchedDependencies": {
        "express@4.18.1": "patches/express@4.18.1.patch"
      },
      "allowNonAppliedPatches": true
    }
  }
  ```

## 8.5.0

### Minor Changes

- c90798461: When `publishConfig.directory` is set, only symlink it to other workspace projects if `publishConfig.linkDirectory` is set to `true`. Otherwise, only use it for publishing [#5115](https://github.com/pnpm/pnpm/issues/5115).

## 8.4.0

### Minor Changes

- 8e5b77ef6: Add PatchFile type.

## 8.3.0

### Minor Changes

- 2a34b21ce: Dependencies patching is possible via the `pnpm.patchedDependencies` field of the `package.json`.
  To patch a package, the package name, exact version, and the relative path to the patch file should be specified. For instance:

  ```json
  {
    "pnpm": {
      "patchedDependencies": {
        "eslint@1.0.0": "./patches/eslint@1.0.0.patch"
      }
    }
  }
  ```

## 8.2.0

### Minor Changes

- fb5bbfd7a: A new setting added: `pnpm.peerDependencyRules.allowAny`. `allowAny` is an array of package name patterns, any peer dependency matching the pattern will be resolved from any version, regardless of the range specified in `peerDependencies`. For instance:

  ```
  {
    "pnpm": {
      "peerDependencyRules": {
        "allowAny": ["@babel/*", "eslint"]
      }
    }
  }
  ```

  The above setting will mute any warnings about peer dependency version mismatches related to `@babel/` packages or `eslint`.

## 8.1.0

### Minor Changes

- 4d39e4a0c: Add new setting to pnpm field of the manifest: allowedDeprecatedVersions.

## 8.0.1

### Patch Changes

- 18ba5e2c0: Add typesVersions to PUBLISH_CONFIG_WHITELIST

## 8.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Minor Changes

- d504dc380: New fields add to package.json type.

## 7.10.0

### Minor Changes

- b138d048c: New optional field supported: `onlyBuiltDependencies`.

## 7.9.0

### Minor Changes

- 26cd01b88: New field added to package.json.pnpm section: peerDependencyRules.

## 7.8.0

### Minor Changes

- b5734a4a7: Add `resolvedFrom` field to `BadPeerDependencyIssues`.

## 7.7.1

### Patch Changes

- 6493e0c93: add readme file to published package.json file

## 7.7.0

### Minor Changes

- ba9b2eba1: Add types for peer dependency issues.

## 7.6.0

### Minor Changes

- 302ae4f6f: Support async hooks

## 7.5.0

### Minor Changes

- 4ab87844a: New optional field added to `dependenciesMeta`: `injected`.

## 7.4.0

### Minor Changes

- b734b45ea: Add `publishConfig.executableFiles`.

## 7.3.0

### Minor Changes

- 8e76690f4: New optional field added to the manifest type (`package.json`): `pnpm.packageExtensions.

## 7.2.0

### Minor Changes

- 724c5abd8: support "publishConfig.directory" field

## 7.1.0

### Minor Changes

- 97c64bae4: An optional `dir` parameter added to the `ReadPackageHook` function. The `dir` parameter is defined when the hook runs on project manifests and defined the root of the project.

## 7.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

## 6.4.0

### Minor Changes

- 9ad8c27bf: Allow to ignore builds of specified dependencies through the `pnpm.neverBuiltDependencies` field in `package.json`.

## 6.3.1

### Patch Changes

- b5d694e7f: Use pnpm.overrides instead of resolutions. Still support resolutions for partial compatibility with Yarn and for avoiding a breaking change.

## 6.3.0

### Minor Changes

- d54043ee4: A new optional field added to the ProjectManifest type: resolutions.

## 6.2.0

### Minor Changes

- db17f6f7b: Add Project and ProjectsGraph types.

## 6.1.0

### Minor Changes

- 71a8c8ce3: Added a new type: HoistedDependencies.

## 6.0.0

### Major Changes

- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.

## 6.0.0-alpha.0

### Major Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
