# @pnpm/types

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

- b77651d14: New setting supported in the `package.json` that is in the root of the workspace: `pnpm.requiredScripts`. Scripts listed in this array will be required in each project of the worksapce. Otherwise, `pnpm -r run <script name>` will fail [#5569](https://github.com/pnpm/pnpm/issues/5569).

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
