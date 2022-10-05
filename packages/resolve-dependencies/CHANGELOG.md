# @pnpm/resolve-dependencies

## 28.4.4

### Patch Changes

- e8a631bf0: When a direct dependency fails to resolve, print the path to the project directory in the error message.
- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/manifest-utils@3.1.5
  - @pnpm/npm-resolver@13.1.10
  - @pnpm/read-package-json@6.0.11

## 28.4.3

### Patch Changes

- ff331dd95: Don't override the root dependency when auto installing peer dependencies [#5412](https://github.com/pnpm/pnpm/issues/5412).
- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/core-loggers@7.0.8
  - dependency-path@9.2.6
  - @pnpm/lockfile-types@4.3.3
  - @pnpm/lockfile-utils@4.2.6
  - @pnpm/manifest-utils@3.1.4
  - @pnpm/npm-resolver@13.1.9
  - @pnpm/pick-registry-for-package@3.0.8
  - @pnpm/prune-lockfile@4.0.16
  - @pnpm/read-package-json@6.0.10
  - @pnpm/resolver-base@9.1.2
  - @pnpm/store-controller-types@14.1.3

## 28.4.2

### Patch Changes

- 77f7cee48: Don't crash when auto-install-peers is true and the project has many complex circular dependencies.

## 28.4.1

### Patch Changes

- a1e834bfc: Deduplicate peer dependencies when automatically installing them [#5373](https://github.com/pnpm/pnpm/issues/5373).

## 28.4.0

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

### Patch Changes

- 8cecfcbe3: When the same dependency with missing peers is used in multiple workspace projects, install the missing peers in each workspace project [#4820](https://github.com/pnpm/pnpm/issues/4820).
- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/core-loggers@7.0.7
  - dependency-path@9.2.5
  - @pnpm/lockfile-types@4.3.2
  - @pnpm/lockfile-utils@4.2.5
  - @pnpm/manifest-utils@3.1.3
  - @pnpm/npm-resolver@13.1.8
  - @pnpm/pick-registry-for-package@3.0.7
  - @pnpm/prune-lockfile@4.0.15
  - @pnpm/read-package-json@6.0.9
  - @pnpm/resolver-base@9.1.1
  - @pnpm/store-controller-types@14.1.2

## 28.3.11

### Patch Changes

- Updated dependencies [a3ccd27a3]
  - @pnpm/npm-resolver@13.1.7

## 28.3.10

### Patch Changes

- 2acf38be3: Auto installing a peer dependency in a workspace that also has it as a dev dependency in another project [#5144](https://github.com/pnpm/pnpm/issues/5144).

## 28.3.9

### Patch Changes

- 0373af22e: Always correctly update the "time" field in "pnpm-lock.yaml".
- Updated dependencies [d7fc07cc7]
  - @pnpm/npm-resolver@13.1.6

## 28.3.8

### Patch Changes

- 829b4d924: Don't fail when publishedBy date cannot be calculated.
- Updated dependencies [7fac3b446]
  - @pnpm/npm-resolver@13.1.5

## 28.3.7

### Patch Changes

- 53506c7ae: Don't modify the manifest of the injected workspace project, when it has the same dependency in prod and peer dependencies.
- Updated dependencies [53506c7ae]
  - @pnpm/npm-resolver@13.1.4

## 28.3.6

### Patch Changes

- dbac0ca01: Update @yarnpkg/core.
- 9faf0221d: Update Yarn dependencies.
- 054b4e062: Replace replace-string with string.prototype.replaceall.
- 071aa1842: When the same package is both in "peerDependencies" and in "dependencies", treat this dependency as a peer dependency if it may be resolved from the dependencies of parent packages [#5210](https://github.com/pnpm/pnpm/pull/5210).
- Updated dependencies [dbac0ca01]
- Updated dependencies [07bc24ad1]
  - @pnpm/npm-resolver@13.1.3
  - @pnpm/read-package-json@6.0.8

## 28.3.5

### Patch Changes

- Updated dependencies [32915f0e4]
- Updated dependencies [23984abd1]
  - @pnpm/store-controller-types@14.1.1
  - @pnpm/resolver-base@9.1.0
  - @pnpm/lockfile-utils@4.2.4
  - @pnpm/npm-resolver@13.1.2

## 28.3.4

### Patch Changes

- Updated dependencies [238a165a5]
  - @pnpm/npm-resolver@13.1.1

## 28.3.3

### Patch Changes

- 0321ca32a: Don't print the same deprecation warning multiple times.
- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
  - @pnpm/lockfile-utils@4.2.3
  - @pnpm/prune-lockfile@4.0.14
  - @pnpm/store-controller-types@14.1.0
  - @pnpm/npm-resolver@13.1.0

## 28.3.2

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/npm-resolver@13.1.0
  - @pnpm/types@8.5.0
  - @pnpm/core-loggers@7.0.6
  - dependency-path@9.2.4
  - @pnpm/lockfile-types@4.3.1
  - @pnpm/lockfile-utils@4.2.2
  - @pnpm/manifest-utils@3.1.2
  - @pnpm/pick-registry-for-package@3.0.6
  - @pnpm/prune-lockfile@4.0.13
  - @pnpm/read-package-json@6.0.7
  - @pnpm/resolver-base@9.0.6
  - @pnpm/store-controller-types@14.0.2

## 28.3.1

### Patch Changes

- Updated dependencies [c83f40c10]
  - @pnpm/lockfile-utils@4.2.1

## 28.3.0

### Minor Changes

- 8dcfbe357: Add `publishDirectory` field to the lockfile and relink the project when it changes.

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-types@4.3.0
  - @pnpm/lockfile-utils@4.2.0
  - @pnpm/prune-lockfile@4.0.12
  - @pnpm/npm-resolver@13.0.7

## 28.2.3

### Patch Changes

- Updated dependencies [eb2426cf8]
  - @pnpm/npm-resolver@13.0.7

## 28.2.2

### Patch Changes

- e3f4d131c: When `auto-install-peers` is set to `true`, automatically install direct peer dependencies [#5028](https://github.com/pnpm/pnpm/pull/5067).

  So if your project the next manifest:

  ```json
  {
    "dependencies": {
      "lodash": "^4.17.21"
    },
    "peerDependencies": {
      "react": "^18.2.0"
    }
  }
  ```

  pnpm will install both lodash and react as a regular dependencies.

- Updated dependencies [e3f4d131c]
- Updated dependencies [e3f4d131c]
  - @pnpm/manifest-utils@3.1.1
  - @pnpm/lockfile-utils@4.1.0

## 28.2.1

### Patch Changes

- 406656f80: When `lockfile-include-tarball-url` is set to `true`, every entry in `pnpm-lock.yaml` will contain the full URL to the package's tarball [#5054](https://github.com/pnpm/pnpm/pull/5054).
  - @pnpm/npm-resolver@13.0.6

## 28.2.0

### Minor Changes

- f5621a42c: A new value `rolling` for option `save-workspace-protocol`. When selected, pnpm will save workspace versions using a rolling alias (e.g. `"foo": "workspace:^"`) instead of pinning the current version number (e.g. `"foo": "workspace:^1.0.0"`). Usage example:

  ```
  pnpm --save-workspace-protocol=rolling add foo
  ```

### Patch Changes

- Updated dependencies [f5621a42c]
  - @pnpm/manifest-utils@3.1.0
  - @pnpm/which-version-is-pinned@3.0.0
  - dependency-path@9.2.3
  - @pnpm/lockfile-utils@4.0.10
  - @pnpm/prune-lockfile@4.0.11

## 28.1.4

### Patch Changes

- 5e0e7f5db: `pnpm install` in a workspace with patches should not fail when doing partial installation [#4954](https://github.com/pnpm/pnpm/issues/4954).

## 28.1.3

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
  - @pnpm/lockfile-utils@4.0.9
  - @pnpm/prune-lockfile@4.0.10

## 28.1.2

### Patch Changes

- fc581d371: Don't fail when the patched package appears multiple times in the dependency graph [#4938](https://github.com/pnpm/pnpm/issues/4938).
- Updated dependencies [fc581d371]
  - dependency-path@9.2.2
  - @pnpm/lockfile-utils@4.0.8
  - @pnpm/prune-lockfile@4.0.9

## 28.1.1

### Patch Changes

- 8e5b77ef6: Update the dependencies when a patch file is modified.
- Updated dependencies [d01c32355]
- Updated dependencies [8e5b77ef6]
- Updated dependencies [8e5b77ef6]
  - @pnpm/lockfile-types@4.2.0
  - @pnpm/types@8.4.0
  - @pnpm/lockfile-utils@4.0.7
  - @pnpm/prune-lockfile@4.0.8
  - @pnpm/core-loggers@7.0.5
  - dependency-path@9.2.1
  - @pnpm/manifest-utils@3.0.6
  - @pnpm/npm-resolver@13.0.6
  - @pnpm/pick-registry-for-package@3.0.5
  - @pnpm/read-package-json@6.0.6
  - @pnpm/resolver-base@9.0.5
  - @pnpm/store-controller-types@14.0.1

## 28.1.0

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

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [c635f9fc1]
- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/lockfile-types@4.1.0
  - dependency-path@9.2.0
  - @pnpm/store-controller-types@14.0.0
  - @pnpm/core-loggers@7.0.4
  - @pnpm/lockfile-utils@4.0.6
  - @pnpm/manifest-utils@3.0.5
  - @pnpm/npm-resolver@13.0.5
  - @pnpm/pick-registry-for-package@3.0.4
  - @pnpm/prune-lockfile@4.0.7
  - @pnpm/read-package-json@6.0.5
  - @pnpm/resolver-base@9.0.4

## 28.0.0

### Major Changes

- 0abfe1718: `requiresBuild` is sometimes a function that return a boolean promise.

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [725636a90]
  - @pnpm/types@8.2.0
  - dependency-path@9.1.4
  - @pnpm/core-loggers@7.0.3
  - @pnpm/lockfile-types@4.0.3
  - @pnpm/lockfile-utils@4.0.5
  - @pnpm/manifest-utils@3.0.4
  - @pnpm/npm-resolver@13.0.4
  - @pnpm/pick-registry-for-package@3.0.3
  - @pnpm/prune-lockfile@4.0.6
  - @pnpm/read-package-json@6.0.4
  - @pnpm/resolver-base@9.0.3
  - @pnpm/store-controller-types@13.0.4

## 27.2.0

### Minor Changes

- 4d39e4a0c: A new setting is supported for ignoring specific deprecation messages: `pnpm.allowedDeprecatedVersions`. The setting should be provided in the `pnpm` section of the root `package.json` file. The below example will mute any deprecation warnings about the `request` package and warnings about `express` v1:

  ```json
  {
    "pnpm": {
      "allowedDeprecatedVersions": {
        "request": "*",
        "express": "1"
      }
    }
  }
  ```

  Related issue: [#4306](https://github.com/pnpm/pnpm/issues/4306)
  Related PR: [#4864](https://github.com/pnpm/pnpm/pull/4864)

### Patch Changes

- 26413c30c: Report only the first occurence of a deprecated package.
- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/core-loggers@7.0.2
  - dependency-path@9.1.3
  - @pnpm/lockfile-types@4.0.2
  - @pnpm/lockfile-utils@4.0.4
  - @pnpm/manifest-utils@3.0.3
  - @pnpm/npm-resolver@13.0.3
  - @pnpm/pick-registry-for-package@3.0.2
  - @pnpm/prune-lockfile@4.0.5
  - @pnpm/read-package-json@6.0.3
  - @pnpm/resolver-base@9.0.2
  - @pnpm/store-controller-types@13.0.3

## 27.1.4

### Patch Changes

- 9f5352014: When the same package is found several times in the dependency graph, correctly autoinstall its missing peer dependencies at all times [#4820](https://github.com/pnpm/pnpm/issues/4820).

## 27.1.3

### Patch Changes

- 6756c2b02: It should be possible to install a git-hosted package that has no `package.json` file [#4822](https://github.com/pnpm/pnpm/issues/4822).
- Updated dependencies [6756c2b02]
  - @pnpm/store-controller-types@13.0.2
  - @pnpm/npm-resolver@13.0.2

## 27.1.2

### Patch Changes

- 2b543c774: Correctly detect repeated dependency sequence during resolution.

## 27.1.1

### Patch Changes

- 45238e358: Don't fail on projects with linked dependencies, when `auto-install-peers` is set to `true` [#4796](https://github.com/pnpm/pnpm/issues/4796).

## 27.1.0

### Minor Changes

- 190f0b331: New option added for automatically installing missing peer dependencies: `autoInstallPeers`.

### Patch Changes

- Updated dependencies [190f0b331]
  - @pnpm/prune-lockfile@4.0.4

## 27.0.4

### Patch Changes

- Updated dependencies [c57695550]
  - dependency-path@9.1.2
  - @pnpm/lockfile-utils@4.0.3
  - @pnpm/prune-lockfile@4.0.3

## 27.0.3

### Patch Changes

- 52b0576af: feat: support libc filed

## 27.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/core-loggers@7.0.1
  - dependency-path@9.1.1
  - @pnpm/lockfile-types@4.0.1
  - @pnpm/lockfile-utils@4.0.2
  - @pnpm/manifest-utils@3.0.2
  - @pnpm/npm-resolver@13.0.2
  - @pnpm/pick-registry-for-package@3.0.1
  - @pnpm/prune-lockfile@4.0.2
  - @pnpm/read-package-json@6.0.2
  - @pnpm/resolver-base@9.0.1
  - @pnpm/store-controller-types@13.0.1

## 27.0.1

### Patch Changes

- 3345c2cce: It should be possible to use a chain of local file dependencies [#4611](https://github.com/pnpm/pnpm/issues/4611).
- 7478cbd05: Installation shouldn't fail when a package from node_modules is moved to the `node_modules/.ignored` subfolder and a package with that name is already present in `node_modules/.ignored'.

## 27.0.0

### Major Changes

- 0a70aedb1: Use a base32 hash instead of a hex to encode too long dependency paths inside `node_modules/.pnpm` [#4552](https://github.com/pnpm/pnpm/pull/4552).
- e7bdc2cc2: Dependencies of the root workspace project are not used to resolve peer dependencies of other workspace projects [#4469](https://github.com/pnpm/pnpm/pull/4469).

### Patch Changes

- 948a8151e: Fix an error with peer resolutions, which was happening when there was a circular dependency and another dependency that had the name of the circular dependency as a substring.
- e531325c3: `dependenciesMeta` should be saved into the lockfile, when it is added to the package manifest by a hook.
- aecd4acdd: Linked in dependencies should be considered when resolving peer dependencies [#4541](https://github.com/pnpm/pnpm/pull/4541).
- dbe366990: Peer dependency should be correctly resolved from the workspace, when it is declared using a workspace protocol [#4529](https://github.com/pnpm/pnpm/issues/4529).
- b716d2d06: Don't update a direct dependency that has the same name as a dependency in the workspace, when adding a new dependency to a workspace project [#4575](https://github.com/pnpm/pnpm/pull/4575).
- Updated dependencies [0a70aedb1]
- Updated dependencies [688b0eaff]
- Updated dependencies [618842b0d]
- Updated dependencies [1267e4eff]
  - dependency-path@9.1.0
  - @pnpm/lockfile-utils@4.0.1
  - @pnpm/manifest-utils@3.0.1
  - @pnpm/constants@6.1.0
  - @pnpm/prune-lockfile@4.0.1
  - @pnpm/error@3.0.1
  - @pnpm/npm-resolver@13.0.1
  - @pnpm/read-package-json@6.0.1

## 26.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.
- 0845a8704: A prerelease version is always added as an exact version to `package.json`. If the `next` version of `foo` is `1.0.0-beta.1` then running `pnpm add foo@next` will add this to `package.json`:

  ```json
  {
    "dependencies": {
      "foo": "1.0.0-beta.1"
    }
  }
  ```

### Patch Changes

- 9b9b13c3a: Update Yarn dependencies.
- Updated dependencies [d504dc380]
- Updated dependencies [faf830b8f]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - dependency-path@9.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/error@3.0.0
  - @pnpm/lockfile-types@4.0.0
  - @pnpm/lockfile-utils@4.0.0
  - @pnpm/manifest-utils@3.0.0
  - @pnpm/npm-resolver@13.0.0
  - @pnpm/pick-registry-for-package@3.0.0
  - @pnpm/prune-lockfile@4.0.0
  - @pnpm/read-package-json@6.0.0
  - @pnpm/resolver-base@9.0.0
  - @pnpm/store-controller-types@13.0.0
  - @pnpm/which-version-is-pinned@2.0.0

## 25.0.2

### Patch Changes

- 4941f31ee: The location of an injected directory dependency should be correctly located, when there is a chain of local dependencies (declared via the `file:` protocol`).

  The next scenario was not working prior to the fix. There are 3 projects in the same folder: foo, bar, qar.

  `foo/package.json`:

  ```json
  {
    "name": "foo",
    "dependencies": {
      "bar": "file:../bar"
    },
    "dependenciesMeta": {
      "bar": {
        "injected": true
      }
    }
  }
  ```

  `bar/package.json`:

  ```json
  {
    "name": "bar",
    "dependencies": {
      "qar": "file:../qar"
    },
    "dependenciesMeta": {
      "qar": {
        "injected": true
      }
    }
  }
  ```

  `qar/package.json`:

  ```json
  {
    "name": "qar"
  }
  ```

  Related PR: [#4415](https://github.com/pnpm/pnpm/pull/4415).

## 25.0.1

### Patch Changes

- 5c525db13: In order to guarantee that only correct data is written to the store, data from the lockfile should not be written to the store. Only data directly from the package tarball or package metadata.
- Updated dependencies [70ba51da9]
- Updated dependencies [5c525db13]
  - @pnpm/error@2.1.0
  - @pnpm/store-controller-types@12.0.0
  - @pnpm/manifest-utils@2.1.9
  - @pnpm/npm-resolver@12.1.8
  - @pnpm/read-package-json@5.0.12

## 25.0.0

### Major Changes

- b138d048c: Removed the `neverBuiltDependencies` option. In order to ignore scripts of some dependencies, use the new `allowBuild`. `allowBuild` is a function that accepts the package name and returns `true` if the package should be allowed to build.

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-types@3.2.0
  - @pnpm/types@7.10.0
  - @pnpm/lockfile-utils@3.2.1
  - @pnpm/prune-lockfile@3.0.15
  - @pnpm/core-loggers@6.1.4
  - dependency-path@8.0.11
  - @pnpm/manifest-utils@2.1.8
  - @pnpm/npm-resolver@12.1.7
  - @pnpm/pick-registry-for-package@2.0.11
  - @pnpm/read-package-json@5.0.11
  - @pnpm/resolver-base@8.1.6
  - @pnpm/store-controller-types@11.0.12

## 24.0.0

### Major Changes

- 37d09a68f: Don't skip a dependency that is named the same way as the package, if it has a different version.

## 23.0.4

### Patch Changes

- Updated dependencies [cdc521cfa]
  - @pnpm/lockfile-utils@3.2.0
  - @pnpm/npm-resolver@12.1.6

## 23.0.3

### Patch Changes

- Updated dependencies [8a2cad034]
  - @pnpm/manifest-utils@2.1.7

## 23.0.2

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/core-loggers@6.1.3
  - dependency-path@8.0.10
  - @pnpm/lockfile-types@3.1.5
  - @pnpm/lockfile-utils@3.1.6
  - @pnpm/manifest-utils@2.1.6
  - @pnpm/npm-resolver@12.1.6
  - @pnpm/pick-registry-for-package@2.0.10
  - @pnpm/prune-lockfile@3.0.14
  - @pnpm/read-package-json@5.0.10
  - @pnpm/resolver-base@8.1.5
  - @pnpm/store-controller-types@11.0.11

## 23.0.1

### Patch Changes

- cbd2f3e2a: Downgrade and pin Yarn lib versions.

## 23.0.0

### Major Changes

- 8ddcd5116: Don't log fetch statuses of packages. This logging was moved to `@pnpm/package-requester`.

## 22.1.0

### Minor Changes

- b5734a4a7: BadPeerDependencyIssue should contain the path to the package that has the dependency from which the peer dependency is resolved.

### Patch Changes

- b390c75a6: Injected subdependencies should be hard linked as well. So if `button` is injected into `card` and `card` is injected into `page`, then both `button` and `card` should be injected into `page`.
- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/core-loggers@6.1.2
  - dependency-path@8.0.9
  - @pnpm/lockfile-types@3.1.4
  - @pnpm/lockfile-utils@3.1.5
  - @pnpm/manifest-utils@2.1.5
  - @pnpm/npm-resolver@12.1.5
  - @pnpm/pick-registry-for-package@2.0.9
  - @pnpm/prune-lockfile@3.0.13
  - @pnpm/read-package-json@5.0.9
  - @pnpm/resolver-base@8.1.4
  - @pnpm/store-controller-types@11.0.10

## 22.0.2

### Patch Changes

- 7962c042e: Don't warn about unmet peer dependency when the peer is resolved from a prerelease version.

  For instance, if a project has `react@*` as a peer dependency, then react `16.0.0-rc.0` should not cause a warning.

## 22.0.1

### Patch Changes

- cb1827b9c: If making an intersection of peer dependency ranges does not succeed, install should not crash [#4134](https://github.com/pnpm/pnpm/issues/4134).
- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/core-loggers@6.1.1
  - dependency-path@8.0.8
  - @pnpm/lockfile-types@3.1.3
  - @pnpm/lockfile-utils@3.1.4
  - @pnpm/manifest-utils@2.1.4
  - @pnpm/npm-resolver@12.1.4
  - @pnpm/pick-registry-for-package@2.0.8
  - @pnpm/prune-lockfile@3.0.12
  - @pnpm/read-package-json@5.0.8
  - @pnpm/resolver-base@8.1.3
  - @pnpm/store-controller-types@11.0.9

## 22.0.0

### Major Changes

- ae32d313e: Breaking changes to the API. New required options added: `defaultUpdateDepth` and `preferredVersions`.

### Minor Changes

- 25f0fa9fa: `resolveDependencies()` should return `peerDependenciesIssues`.

### Patch Changes

- 5af305f39: Installation should be finished before an error about bad/missing peer dependencies is printed and kills the process.
- a626c60fc: When `strict-peer-dependencies` is used, don't fail on the first peer dependency issue. Print all the peer dependency issues and then stop the installation process [#4082](https://github.com/pnpm/pnpm/pull/4082).
- Updated dependencies [ae32d313e]
- Updated dependencies [ba9b2eba1]
- Updated dependencies [ba9b2eba1]
- Updated dependencies [81ed15666]
  - @pnpm/which-version-is-pinned@1.0.0
  - @pnpm/core-loggers@6.1.0
  - @pnpm/types@7.7.0
  - @pnpm/npm-resolver@12.1.3
  - @pnpm/manifest-utils@2.1.3
  - dependency-path@8.0.7
  - @pnpm/lockfile-types@3.1.2
  - @pnpm/lockfile-utils@3.1.3
  - @pnpm/pick-registry-for-package@2.0.7
  - @pnpm/prune-lockfile@3.0.11
  - @pnpm/read-package-json@5.0.7
  - @pnpm/resolver-base@8.1.2
  - @pnpm/store-controller-types@11.0.8

## 21.2.3

### Patch Changes

- 3cf543fc1: Non-standard tarball URL should be correctly calculated when the registry has no traling slash in the configuration file [#4052](https://github.com/pnpm/pnpm/issues/4052). This is a regression caused introduced in v6.23.2 caused by [#4032](https://github.com/pnpm/pnpm/pull/4032).
- Updated dependencies [3cf543fc1]
  - @pnpm/lockfile-utils@3.1.2

## 21.2.2

### Patch Changes

- Updated dependencies [9f61bd81b]
  - @pnpm/npm-resolver@12.1.2

## 21.2.1

### Patch Changes

- 828e3b9e4: `peerDependencies` ranges should be compared loosely [#3753](https://github.com/pnpm/pnpm/issues/3753).

## 21.2.0

### Minor Changes

- 302ae4f6f: Support async hooks

### Patch Changes

- 108bd4a39: Injected directory resolutions should contain the relative path to the directory.
- Updated dependencies [302ae4f6f]
- Updated dependencies [108bd4a39]
  - @pnpm/types@7.6.0
  - @pnpm/npm-resolver@12.1.1
  - @pnpm/core-loggers@6.0.6
  - dependency-path@8.0.6
  - @pnpm/lockfile-types@3.1.1
  - @pnpm/lockfile-utils@3.1.1
  - @pnpm/manifest-utils@2.1.2
  - @pnpm/pick-registry-for-package@2.0.6
  - @pnpm/prune-lockfile@3.0.10
  - @pnpm/read-package-json@5.0.6
  - @pnpm/resolver-base@8.1.1
  - @pnpm/store-controller-types@11.0.7

## 21.1.1

### Patch Changes

- bc1c2aa62: The `dependenciesMeta` field should be added to all packages that have it in the manifest.

## 21.1.0

### Minor Changes

- 4ab87844a: Added support for "injected" dependencies.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/npm-resolver@12.1.0
  - @pnpm/resolver-base@8.1.0
  - @pnpm/lockfile-types@3.1.0
  - @pnpm/lockfile-utils@3.1.0
  - @pnpm/core-loggers@6.0.5
  - dependency-path@8.0.5
  - @pnpm/manifest-utils@2.1.1
  - @pnpm/pick-registry-for-package@2.0.5
  - @pnpm/prune-lockfile@3.0.9
  - @pnpm/read-package-json@5.0.5
  - @pnpm/store-controller-types@11.0.6

## 21.0.7

### Patch Changes

- Updated dependencies [82caa0b56]
  - @pnpm/npm-resolver@12.0.5

## 21.0.6

### Patch Changes

- 4b163f69c: Dedupe dependencies when one of the packages is updated or a new one is added.

## 21.0.5

### Patch Changes

- Updated dependencies [553a5d840]
  - @pnpm/manifest-utils@2.1.0

## 21.0.4

### Patch Changes

- 11a934da1: `requiresBuild` fields should be updated when a full resolution is forced.
  - @pnpm/npm-resolver@12.0.4

## 21.0.3

### Patch Changes

- Updated dependencies [eadf0e505]
  - @pnpm/npm-resolver@12.0.3

## 21.0.2

### Patch Changes

- ee589ab9b: Installation should not fail if a non-optional dependency of a skipped dependency is not installable.

## 21.0.1

### Patch Changes

- 31e01d9a9: Fetch a package if it is not installable as optional but also exists as not optional.

## 21.0.0

### Major Changes

- 07e7b1c0c: Optional dependencies are always marked as `requiresBuild` as they are not always fetched and as a result there is no way to check whether they need to be built or not.

## 20.0.16

### Patch Changes

- Updated dependencies [a4fed2798]
  - @pnpm/npm-resolver@12.0.2

## 20.0.15

### Patch Changes

- 135d53827: Include the path to the project in which the peer dependency is missing.

## 20.0.14

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/core-loggers@6.0.4
  - dependency-path@8.0.4
  - @pnpm/lockfile-utils@3.0.8
  - @pnpm/manifest-utils@2.0.4
  - @pnpm/npm-resolver@12.0.1
  - @pnpm/package-is-installable@5.0.4
  - @pnpm/pick-registry-for-package@2.0.4
  - @pnpm/prune-lockfile@3.0.8
  - @pnpm/read-package-json@5.0.4
  - @pnpm/resolver-base@8.0.4
  - @pnpm/store-controller-types@11.0.5

## 20.0.13

### Patch Changes

- Updated dependencies [691f64713]
  - @pnpm/npm-resolver@12.0.0

## 20.0.12

### Patch Changes

- 389858509: Dependencies from the root workspace package should be used to resolve peer dependencies of any projects in the workspace.

## 20.0.11

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/core-loggers@6.0.3
  - dependency-path@8.0.3
  - @pnpm/lockfile-utils@3.0.7
  - @pnpm/manifest-utils@2.0.3
  - @pnpm/npm-resolver@11.1.4
  - @pnpm/package-is-installable@5.0.3
  - @pnpm/pick-registry-for-package@2.0.3
  - @pnpm/prune-lockfile@3.0.7
  - @pnpm/read-package-json@5.0.3
  - @pnpm/resolver-base@8.0.3
  - @pnpm/store-controller-types@11.0.4

## 20.0.10

### Patch Changes

- c1cdc0184: Peer dependencies should get resolved from the workspace root.
- 060c73677: Use the real package names of the peer dependencies, when creating the paths in the virtual store.
- Updated dependencies [6c418943c]
  - dependency-path@8.0.2
  - @pnpm/lockfile-utils@3.0.6
  - @pnpm/prune-lockfile@3.0.6

## 20.0.9

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/npm-resolver@11.1.3
  - @pnpm/core-loggers@6.0.2
  - dependency-path@8.0.1
  - @pnpm/lockfile-utils@3.0.5
  - @pnpm/manifest-utils@2.0.2
  - @pnpm/package-is-installable@5.0.2
  - @pnpm/pick-registry-for-package@2.0.2
  - @pnpm/prune-lockfile@3.0.5
  - @pnpm/read-package-json@5.0.2
  - @pnpm/resolver-base@8.0.2
  - @pnpm/store-controller-types@11.0.3

## 20.0.8

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/lockfile-utils@3.0.4
  - @pnpm/prune-lockfile@3.0.4

## 20.0.7

### Patch Changes

- Updated dependencies [20e2f235d]
- Updated dependencies [ae36ac7d3]
- Updated dependencies [bf322c702]
  - dependency-path@8.0.0
  - @pnpm/npm-resolver@11.1.2
  - @pnpm/lockfile-utils@3.0.3
  - @pnpm/prune-lockfile@3.0.3

## 20.0.6

### Patch Changes

- @pnpm/npm-resolver@11.1.1

## 20.0.5

### Patch Changes

- @pnpm/store-controller-types@11.0.2

## 20.0.4

### Patch Changes

- 787b69908: Fixing a regression introduced in 20.0.3

## 20.0.3

### Patch Changes

- Updated dependencies [85fb21a83]
- Updated dependencies [05baaa6e7]
- Updated dependencies [97c64bae4]
  - @pnpm/npm-resolver@11.1.0
  - @pnpm/types@7.1.0
  - @pnpm/core-loggers@6.0.1
  - dependency-path@7.0.1
  - @pnpm/lockfile-utils@3.0.2
  - @pnpm/manifest-utils@2.0.1
  - @pnpm/package-is-installable@5.0.1
  - @pnpm/pick-registry-for-package@2.0.1
  - @pnpm/prune-lockfile@3.0.2
  - @pnpm/read-package-json@5.0.1
  - @pnpm/resolver-base@8.0.1
  - @pnpm/store-controller-types@11.0.1

## 20.0.2

### Patch Changes

- Updated dependencies [6f198457d]
  - @pnpm/npm-resolver@11.0.1

## 20.0.1

### Patch Changes

- Updated dependencies [9ceab68f0]
  - dependency-path@7.0.0
  - @pnpm/lockfile-utils@3.0.1
  - @pnpm/prune-lockfile@3.0.1

## 20.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Minor Changes

- 6871d74b2: Add new transitivePeerDependencies field to lockfile.

### Patch Changes

- Updated dependencies [6871d74b2]
- Updated dependencies [97b986fbc]
- Updated dependencies [6871d74b2]
- Updated dependencies [90487a3a8]
- Updated dependencies [e4efddbd2]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [83645c8ed]
  - @pnpm/constants@5.0.0
  - @pnpm/core-loggers@6.0.0
  - dependency-path@6.0.0
  - @pnpm/error@2.0.0
  - @pnpm/lockfile-types@3.0.0
  - @pnpm/lockfile-utils@3.0.0
  - @pnpm/manifest-utils@2.0.0
  - @pnpm/npm-resolver@11.0.0
  - @pnpm/package-is-installable@5.0.0
  - @pnpm/pick-registry-for-package@2.0.0
  - @pnpm/prune-lockfile@3.0.0
  - @pnpm/read-package-json@5.0.0
  - @pnpm/resolver-base@8.0.0
  - @pnpm/store-controller-types@11.0.0
  - @pnpm/types@7.0.0

## 19.0.2

### Patch Changes

- Updated dependencies [d853fb14a]
  - @pnpm/read-package-json@4.0.0

## 19.0.1

### Patch Changes

- @pnpm/npm-resolver@10.2.2

## 19.0.0

### Major Changes

- 8d1dfa89c: Breaking changes to the store controller API.

  The options to `requestPackage()` and `fetchPackage()` changed.

### Patch Changes

- Updated dependencies [8d1dfa89c]
  - @pnpm/store-controller-types@10.0.0

## 18.3.3

### Patch Changes

- ef1588413: `requestPackage()` should always return the resolution of the updated package.

## 18.3.2

### Patch Changes

- 249c068dd: fix scoped registry for aliased dependency
- Updated dependencies [249c068dd]
  - @pnpm/pick-registry-for-package@1.1.0

## 18.3.1

### Patch Changes

- 7578a5ad4: The lockfile needs to be updated when the value of neverBuiltDependencies changes.

## 18.3.0

### Minor Changes

- 9ad8c27bf: New option added for ignore scripts in specified dependencies: `neverBuiltDependencies`.

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [9ad8c27bf]
  - @pnpm/lockfile-types@2.2.0
  - @pnpm/types@6.4.0
  - @pnpm/lockfile-utils@2.0.22
  - @pnpm/prune-lockfile@2.0.19
  - @pnpm/core-loggers@5.0.3
  - dependency-path@5.1.1
  - @pnpm/manifest-utils@1.1.5
  - @pnpm/npm-resolver@10.2.2
  - @pnpm/package-is-installable@4.0.19
  - @pnpm/pick-registry-for-package@1.0.6
  - @pnpm/read-package-json@3.1.9
  - @pnpm/resolver-base@7.1.1
  - @pnpm/store-controller-types@9.2.1

## 18.2.6

### Patch Changes

- e665f5105: The workspace protocol should work in subdependencies.

## 18.2.5

### Patch Changes

- db0c7e157: When a new peer dependency is installed, don't remove the existing regular dependencies of the package that depends on the peer.
- 4d64969a6: Update version-selector-type to v3.

## 18.2.4

### Patch Changes

- Updated dependencies [e27dcf0dc]
  - dependency-path@5.1.0
  - @pnpm/lockfile-utils@2.0.21
  - @pnpm/prune-lockfile@2.0.18

## 18.2.3

### Patch Changes

- Updated dependencies [f47551a3c]
  - @pnpm/npm-resolver@10.2.1

## 18.2.2

### Patch Changes

- @pnpm/npm-resolver@10.2.0

## 18.2.1

### Patch Changes

- @pnpm/npm-resolver@10.2.0

## 18.2.0

### Minor Changes

- 8698a7060: New option added: preferWorkspacePackages. When it is `true`, dependencies are linked from the workspace even, when there are newer version available in the registry.

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/npm-resolver@10.2.0
  - @pnpm/resolver-base@7.1.0
  - @pnpm/store-controller-types@9.2.0
  - @pnpm/lockfile-utils@2.0.20

## 18.1.4

### Patch Changes

- Updated dependencies [284e95c5e]
- Updated dependencies [084614f55]
  - @pnpm/npm-resolver@10.1.0

## 18.1.3

### Patch Changes

- Updated dependencies [5ff6c28fa]
- Updated dependencies [0c5f1bcc9]
  - @pnpm/npm-resolver@10.0.7
  - @pnpm/error@1.4.0
  - @pnpm/manifest-utils@1.1.4
  - @pnpm/package-is-installable@4.0.18
  - @pnpm/read-package-json@3.1.8

## 18.1.2

### Patch Changes

- 39142e2ad: Update encode-registry to v3.
- Updated dependencies [39142e2ad]
  - dependency-path@5.0.6
  - @pnpm/npm-resolver@10.0.6
  - @pnpm/lockfile-utils@2.0.19
  - @pnpm/prune-lockfile@2.0.17

## 18.1.1

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/lockfile-types@2.1.1
  - @pnpm/types@6.3.1
  - @pnpm/lockfile-utils@2.0.18
  - @pnpm/prune-lockfile@2.0.16
  - @pnpm/core-loggers@5.0.2
  - dependency-path@5.0.5
  - @pnpm/manifest-utils@1.1.3
  - @pnpm/npm-resolver@10.0.5
  - @pnpm/package-is-installable@4.0.17
  - @pnpm/pick-registry-for-package@1.0.5
  - @pnpm/read-package-json@3.1.7
  - @pnpm/resolver-base@7.0.5
  - @pnpm/store-controller-types@9.1.2

## 18.1.0

### Minor Changes

- fcdad632f: When some of the dependencies of a package have the package as a peer dependency, don't make the dependency a peer dependency of itself.

### Patch Changes

- d54043ee4: When the version in the lockfile doesn't satisfy the range in the dependency's manifest, re-resolve the dependency.
- Updated dependencies [d54043ee4]
- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
- Updated dependencies [212671848]
  - @pnpm/lockfile-types@2.1.0
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - @pnpm/read-package-json@3.1.6
  - @pnpm/lockfile-utils@2.0.17
  - @pnpm/prune-lockfile@2.0.15
  - @pnpm/core-loggers@5.0.1
  - dependency-path@5.0.4
  - @pnpm/manifest-utils@1.1.2
  - @pnpm/npm-resolver@10.0.4
  - @pnpm/package-is-installable@4.0.16
  - @pnpm/pick-registry-for-package@1.0.4
  - @pnpm/resolver-base@7.0.4
  - @pnpm/store-controller-types@9.1.1

## 18.0.6

### Patch Changes

- 4241bc148: When a peer dependency is not resolved but is available through `require()`, don't print a warning but still consider it to be missing.
- bde7cd164: Peer dependencies should get correctly resolved even in optional dependencies that will be skipped on the active system.
- 9f003e94f: Don't cache the peer resolution of packages that have missing peer dependencies.
- e8dcc42d5: Do not skip a package's peer resolution if it was previously resolved w/o peer dependencies but in the new node it has peer dependencies.
- c6eaf01c9: Resolved peer dependencies should always be included.

## 18.0.5

### Patch Changes

- ddd98dd74: The lockfile should be correctly updated when a direct dependency that has peer dependencies has a new version specifier in `package.json`.

  For instance, `jest@26` has `cascade@2` in its peer dependencies. So `pnpm install` will scope Jest to some version of cascade. This is how it will look like in `pnpm-lock.yaml`:

  ```yaml
  dependencies:
    canvas: 2.6.0
    jest: 26.4.0_canvas@2.6.0
  ```

  If the version specifier of Jest gets changed in the `package.json` to `26.5.0`, the next time `pnpm install` is executed, the lockfile should be changed to this:

  ```yaml
  dependencies:
    canvas: 2.6.0
    jest: 26.5.0_canvas@2.6.0
  ```

  Prior to this fix, after the update, Jest was not scoped with canvas, so the lockfile was incorrectly updated to the following:

  ```yaml
  dependencies:
    canvas: 2.6.0
    jest: 26.5.0
  ```

  Related issue: [#2919](https://github.com/pnpm/pnpm/issues/2919).
  Related PR: [#2920](https://github.com/pnpm/pnpm/pull/2920).

## 18.0.4

### Patch Changes

- Updated dependencies [d7b727795]
  - @pnpm/npm-resolver@10.0.3

## 18.0.3

### Patch Changes

- Updated dependencies [0a6544043]
  - @pnpm/store-controller-types@9.1.0

## 18.0.2

### Patch Changes

- Updated dependencies [86cd72de3]
- Updated dependencies [3633f5e46]
- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0
  - @pnpm/npm-resolver@10.0.2
  - @pnpm/store-controller-types@9.0.0
  - @pnpm/manifest-utils@1.1.1
  - @pnpm/package-is-installable@4.0.15

## 18.0.1

### Patch Changes

- @pnpm/npm-resolver@10.0.1

## 18.0.0

### Major Changes

- e2f6b40b1: Breaking changes to the API. `resolveDependencies()` now returns a dependency graph with peer dependencies resolved.

### Patch Changes

- Updated dependencies [e2f6b40b1]
- Updated dependencies [e2f6b40b1]
- Updated dependencies [e2f6b40b1]
  - @pnpm/manifest-utils@1.1.0

## 17.0.0

### Major Changes

- 9d9456442: In case of leaf dependencies (dependencies that have no prod deps or peer deps), we only ever need to analyze one leaf dep in a graph, so the nodeId can be short and stateless, like the package ID.
- 501efdabd: Use depPath in nodeIds instead of package IDs (depPath is unique as well but shorter).
- 501efdabd: `resolvedPackagesByPackageId` is replaced with `resolvedPackagesByDepPath`.

### Minor Changes

- a43c12afe: We are building the dependency tree only until there are new packages or the packages repeat in a unique order. This is needed later during peer dependencies resolution.

  So we resolve `foo > bar > qar > foo`.
  But we stop on `foo > bar > qar > foo > qar`.
  In the second example, there's no reason to walk qar again when qar is included the first time, the dependencies of foo are already resolved and included as parent dependencies of qar. So during peers resolution, qar cannot possibly get any new or different peers resolved, after the first occurrence.

  However, in the next example we would analyze the second qar as well, because zoo is a new parent package:
  `foo > bar > qar > zoo > qar`

## 16.1.5

### Patch Changes

- 8242401c7: Ignore non-array bundle\[d]Dependencies fields. Fixes a regression caused by https://github.com/pnpm/pnpm/commit/5322cf9b39f637536aa4775aa64dd4e9a4156d8a

## 16.1.4

### Patch Changes

- Updated dependencies [75a36deba]
- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/npm-resolver@10.0.1
  - @pnpm/package-is-installable@4.0.14

## 16.1.3

### Patch Changes

- Updated dependencies [a1cdae3dc]
  - @pnpm/npm-resolver@10.0.0

## 16.1.2

### Patch Changes

- Updated dependencies [6d480dd7a]
- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/npm-resolver@9.1.0
  - @pnpm/package-is-installable@4.0.13

## 16.1.1

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [622c0b6f9]
- Updated dependencies [1140ef721]
- Updated dependencies [a2ef8084f]
  - @pnpm/npm-resolver@9.0.2
  - @pnpm/lockfile-utils@2.0.16
  - dependency-path@5.0.3

## 16.1.0

### Minor Changes

- 8c1cf25b7: New option added: updateMatching. updateMatching is a function that accepts a package name. It returns `true` if the specified package should be updated.

## 16.0.6

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0
  - @pnpm/package-is-installable@4.0.12
  - @pnpm/npm-resolver@9.0.1

## 16.0.5

### Patch Changes

- Updated dependencies [379cdcaf8]
  - @pnpm/npm-resolver@9.0.1

## 16.0.4

### Patch Changes

- 7f25dad04: Only add packages to the skipped set, when they are seen the first time.

## 16.0.3

### Patch Changes

- Updated dependencies [71aeb9a38]
  - @pnpm/npm-resolver@9.0.0

## 16.0.2

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - dependency-path@5.0.2
  - @pnpm/lockfile-utils@2.0.15
  - @pnpm/npm-resolver@8.1.2
  - @pnpm/package-is-installable@4.0.11
  - @pnpm/pick-registry-for-package@1.0.3
  - @pnpm/resolver-base@7.0.3
  - @pnpm/store-controller-types@8.0.2

## 16.0.1

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/core-loggers@4.1.1
  - dependency-path@5.0.1
  - @pnpm/lockfile-utils@2.0.14
  - @pnpm/npm-resolver@8.1.1
  - @pnpm/package-is-installable@4.0.10
  - @pnpm/pick-registry-for-package@1.0.2
  - @pnpm/resolver-base@7.0.2
  - @pnpm/store-controller-types@8.0.1

## 16.0.0

### Major Changes

- 41d92948b: Expects direct tarball IDs to start with @.

### Patch Changes

- Updated dependencies [41d92948b]
  - dependency-path@5.0.0
  - @pnpm/lockfile-utils@2.0.13

## 15.1.2

### Patch Changes

- Updated dependencies [4cf7ef367]
- Updated dependencies [d3ddd023c]
- Updated dependencies [2ebb7af33]
  - @pnpm/npm-resolver@8.1.0
  - @pnpm/core-loggers@4.1.0
  - @pnpm/package-is-installable@4.0.9

## 15.1.1

### Patch Changes

- @pnpm/npm-resolver@8.0.1

## 15.1.0

### Minor Changes

- 71b0cb8fd: A new option added: `forceFullResolution`. When `true`, the whole dependency graph will be walked through during resolution.

## 15.0.1

### Patch Changes

- e2c4fdad5: Don't remove resolved peer dependencies from dependencies when lockfile is partially up-to-date.

## 15.0.0

### Major Changes

- 0730bb938: Check the existence of a dependency in `node_modules` at the right location.
- 242cf8737: The `alwaysTryWorkspacePackages` option is removed. A new option called `linkWorkspacePackagesDepth` is added.
  When `linkWorkspacePackageDepth` is `0`, workspace packages are linked to direct dependencies even if these direct
  dependencies are not using workspace ranges (so this is similar to the old `alwaysTryWorkspacePackages=true`).
  `linkWorkspacePackageDepth` also allows to link workspace packages to subdependencies by setting the max depth.
  Setting it to `Infinity` will make the resolution algorithm always prefer packages from the workspace over packages
  from the registry.
- cc8a3bd31: `updateLockfile` options property is removed. `updateDepth=Infinity` should be used instead. Which is set for each project separately.
- 16d1ac0fd: `engineCache` is removed from `ResolvedPackage`. `sideEffectsCache` removed from input options.
- e11019b89: Deprecate the resolution strategy setting. The fewer dependencies strategy is used always.
- 802d145fc: Remove `independent-leaves` support.
- 9fbb74ecb: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Minor Changes

- a5febb913: Package request response contains the path to the files index file.
- b47f9737a: When direct dependencies are present, subdependencies are not reanalyzed on repeat install.

### Patch Changes

- 77bc9b510: Resolve subdependencies only after all parent dependencies were resolved.
- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- 4cc0ead24: Update replace-string to v3.1.0.
- c25cccdad: The lockfile should be recreated correctly when an up-to-date `node_modules` is present.
  The recreated lockfile should contain all the skipped optional dependencies.
- Updated dependencies [5bc033c43]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [f516d266c]
- Updated dependencies [da091c711]
- Updated dependencies [42e6490d1]
- Updated dependencies [a5febb913]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [6a8a97eee]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [f453a5f46]
  - @pnpm/npm-resolver@8.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lockfile-types@2.0.1
  - @pnpm/core-loggers@4.0.2
  - dependency-path@4.0.7
  - @pnpm/error@1.2.1
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/package-is-installable@4.0.8
  - @pnpm/pick-registry-for-package@1.0.1
  - @pnpm/resolver-base@7.0.1

## 15.0.0-alpha.6

### Major Changes

- 242cf8737: The `alwaysTryWorkspacePackages` option is removed. A new option called `linkWorkspacePackagesDepth` is added.
  When `linkWorkspacePackageDepth` is `0`, workspace packages are linked to direct dependencies even if these direct
  dependencies are not using workspace ranges (so this is similar to the old `alwaysTryWorkspacePackages=true`).
  `linkWorkspacePackageDepth` also allows to link workspace packages to subdependencies by setting the max depth.
  Setting it to `Infinity` will make the resolution algorithm always prefer packages from the workspace over packages
  from the registry.
- cc8a3bd31: `updateLockfile` options property is removed. `updateDepth=Infinity` should be used instead. Which is set for each project separately.
- 16d1ac0fd: `engineCache` is removed from `ResolvedPackage`. `sideEffectsCache` removed from input options.

### Minor Changes

- a5febb913: Package request response contains the path to the files index file.
- b47f9737a: When direct dependencies are present, subdependencies are not reanalyzed on repeat install.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- c25cccdad: The lockfile should be recreated correctly when an up-to-date `node_modules` is present.
  The recreated lockfile should contain all the skipped optional dependencies.
- Updated dependencies [16d1ac0fd]
- Updated dependencies [a5febb913]
- Updated dependencies [6a8a97eee]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/lockfile-types@2.0.1-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.1

## 15.0.0-alpha.5

### Major Changes

- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- 4cc0ead2: Update replace-string to v3.1.0.
- Updated dependencies [da091c71]
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/core-loggers@4.0.2-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/npm-resolver@7.3.12-alpha.2
  - @pnpm/package-is-installable@4.0.8-alpha.0
  - @pnpm/pick-registry-for-package@1.0.1-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0

## 14.4.5-alpha.4

### Patch Changes

- 0730bb938: Check the existence of a dependency in `node_modules` at the right location.

## 14.4.5-alpha.3

### Patch Changes

- Updated dependencies [5bc033c43]
  - @pnpm/npm-resolver@8.0.0-alpha.1

## 14.4.5-alpha.2

### Patch Changes

- Updated dependencies [42e6490d1]
- Updated dependencies [f453a5f46]
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/npm-resolver@7.3.12-alpha.0

## 14.4.5-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/store-controller-types@8.0.0-alpha.1

## 14.4.5-alpha.0

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/store-controller-types@8.0.0-alpha.0

## 14.4.4

### Patch Changes

- Updated dependencies [907c63a48]
  - @pnpm/lockfile-utils@2.0.11
