# @pnpm/resolve-dependencies

## 34.0.0

### Major Changes

- dd00eeb: Renamed dir to rootDir in the Project object.

### Patch Changes

- 9bf9f71: When encountering an external dependency using the `catalog:` protocol, a clearer error will be shown. Previously a confusing `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` error was thrown. The new error message will explain that the author of the dependency needs to run `pnpm publish` to replace the catalog protocol.
- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/resolver-base@13.0.0
  - @pnpm/npm-resolver@21.0.0
  - @pnpm/types@11.0.0
  - @pnpm/pick-fetcher@3.0.0
  - @pnpm/lockfile-utils@11.0.3
  - @pnpm/lockfile.preferred-versions@1.0.7
  - @pnpm/store-controller-types@18.1.2
  - @pnpm/pick-registry-for-package@6.0.3
  - @pnpm/lockfile-types@7.1.2
  - @pnpm/prune-lockfile@6.1.3
  - @pnpm/core-loggers@10.0.3
  - @pnpm/dependency-path@5.1.2
  - @pnpm/manifest-utils@6.0.4
  - @pnpm/read-package-json@9.0.4

## 33.1.1

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/pick-registry-for-package@6.0.2
  - @pnpm/lockfile-types@7.1.1
  - @pnpm/lockfile-utils@11.0.2
  - @pnpm/lockfile.preferred-versions@1.0.6
  - @pnpm/prune-lockfile@6.1.2
  - @pnpm/core-loggers@10.0.2
  - @pnpm/dependency-path@5.1.1
  - @pnpm/manifest-utils@6.0.3
  - @pnpm/read-package-json@9.0.3
  - @pnpm/npm-resolver@20.0.1
  - @pnpm/resolver-base@12.0.2
  - @pnpm/store-controller-types@18.1.1
  - @pnpm/pick-fetcher@3.0.0

## 33.1.0

### Minor Changes

- 47341e5: **Semi-breaking.** Dependency key names in the lockfile are shortened if they are longer than 1000 characters. We don't expect this change to affect many users. Affected users most probably can't run install successfully at the moment. This change is required to fix some edge cases in which installation fails with an out-of-memory error or "Invalid string length (RangeError: Invalid string length)" error. The max allowed length of the dependency key can be controlled with the `peers-suffix-max-length` setting [#8177](https://github.com/pnpm/pnpm/pull/8177).

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/dependency-path@5.1.0
  - @pnpm/lockfile-types@7.1.0
  - @pnpm/lockfile-utils@11.0.1
  - @pnpm/prune-lockfile@6.1.1
  - @pnpm/lockfile.preferred-versions@1.0.5
  - @pnpm/npm-resolver@20.0.0

## 33.0.4

### Patch Changes

- 74c1057: Improved the performance of the resolution stage by changing how missing peer dependencies are detected [#8144](https://github.com/pnpm/pnpm/pull/8144).

## 33.0.3

### Patch Changes

- 4b65113: Temporary fix. Don't hoist peer dependencies, when peers deduplication is on.

## 33.0.2

### Patch Changes

- 81d90c9: Reduce memory usage during peer dependency resolution by using numbers for Node IDs.
- 27c33f0: Fix a bug in which a dependency that is both optional for one package but non-optional for another is omitted when `optional=false` [#8066](https://github.com/pnpm/pnpm/issues/8066).
- Updated dependencies [27c33f0]
  - @pnpm/prune-lockfile@6.1.0

## 33.0.1

### Patch Changes

- Updated dependencies [0c08e1c]
- Updated dependencies [0c08e1c]
  - @pnpm/npm-resolver@20.0.0
  - @pnpm/store-controller-types@18.1.0

## 33.0.0

### Major Changes

- Breaking changes to the API.

### Patch Changes

- ef73c19: Decrease memory consumption [#8084](https://github.com/pnpm/pnpm/pull/8084).
- 471ee65: Reduce memory usage by peer dependencies resolution [#8072](https://github.com/pnpm/pnpm/issues/8072).
- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-types@7.0.0
  - @pnpm/lockfile-utils@11.0.0
  - @pnpm/dependency-path@5.0.0
  - @pnpm/pick-registry-for-package@6.0.1
  - @pnpm/lockfile.preferred-versions@1.0.4
  - @pnpm/prune-lockfile@6.0.2
  - @pnpm/core-loggers@10.0.1
  - @pnpm/manifest-utils@6.0.2
  - @pnpm/read-package-json@9.0.2
  - @pnpm/npm-resolver@19.0.4
  - @pnpm/resolver-base@12.0.1
  - @pnpm/store-controller-types@18.0.1
  - @pnpm/pick-fetcher@3.0.0

## 32.1.3

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/manifest-utils@6.0.1
  - @pnpm/read-package-json@9.0.1
  - @pnpm/npm-resolver@19.0.3
  - @pnpm/lockfile.preferred-versions@1.0.3

## 32.1.2

### Patch Changes

- 2cb67d7: Improve the performance of the peers resolution stage by utilizing more cache [#8058](https://github.com/pnpm/pnpm/pull/8058).
- Updated dependencies [43b6bb7]
  - @pnpm/npm-resolver@19.0.2

## 32.1.1

### Patch Changes

- 7a0536e: Fix `Cannot read properties of undefined (reading 'missingPeersOfChildren')` exception that happens on install [#8041](https://github.com/pnpm/pnpm/issues/8041).
- cb0f459: `pnpm update` should not fail when there's an aliased local workspace dependency [#7975](https://github.com/pnpm/pnpm/issues/7975).
- Updated dependencies [cb0f459]
- Updated dependencies [7a0536e]
- Updated dependencies [cb0f459]
  - @pnpm/workspace.spec-parser@1.0.0
  - @pnpm/lockfile-utils@10.1.1
  - @pnpm/npm-resolver@19.0.1
  - @pnpm/lockfile.preferred-versions@1.0.2

## 32.1.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- 1a6f7fb: A dependency is hoisted to resolve an optional peer dependency only if it satisfies the range provided for the optional peer dependency [#8028](https://github.com/pnpm/pnpm/pull/8028).
- Updated dependencies [9719a42]
  - @pnpm/dependency-path@4.0.0
  - @pnpm/lockfile-utils@10.1.0
  - @pnpm/prune-lockfile@6.0.1
  - @pnpm/lockfile.preferred-versions@1.0.1
  - @pnpm/npm-resolver@19.0.0

## 32.0.4

### Patch Changes

- abaf12e: Resolve peer dependencies correctly, when they have prerelease versions [#7977](https://github.com/pnpm/pnpm/issues/7977).
- e9530a8: Fix aliased dependencies resolution on repeat install with existing lockfile, when the aliased dependency doesn't specify a version or range [#7957](https://github.com/pnpm/pnpm/issues/7957).

## 32.0.3

### Patch Changes

- eb19475: Fix aliased dependencies resolution on repeat install with existing lockfile [#7957](https://github.com/pnpm/pnpm/issues/7957).

## 32.0.2

### Patch Changes

- b3961cb: Fixed an issue where optional dependencies were not linked into the dependent's node_modules [#7943](https://github.com/pnpm/pnpm/issues/7943).

## 32.0.1

### Patch Changes

- 253d50c: Optional peer dependencies should be resolved as optional dependencies [#7918](https://github.com/pnpm/pnpm/pull/7918).

## 32.0.0

### Major Changes

- cdd8365: Package ID does not contain the registry domain.
- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- 98a1266: Peer dependencies of peer dependencies are now resolved correctly. When peer dependencies have peer dependencies of their own, the peer dependencies are grouped with their own peer dependencies before being linked to their dependents.

  For instance, if `card` has `react` in peer dependencies and `react` has `typescript` in its peer dependencies, then the same version of `react` may be linked from different places if there are multiple versions of `typescript`. For instance:

  ```
  project1/package.json
  {
    "dependencies": {
      "card": "1.0.0",
      "react": "16.8.0",
      "typescript": "7.0.0"
    }
  }
  project2/package.json
  {
    "dependencies": {
      "card": "1.0.0",
      "react": "16.8.0",
      "typescript": "8.0.0"
    }
  }
  node_modules
    .pnpm
      card@1.0.0(react@16.8.0(typescript@7.0.0))
        node_modules
          card
          react --> ../../react@16.8.0(typescript@7.0.0)/node_modules/react
      react@16.8.0(typescript@7.0.0)
        node_modules
          react
          typescript --> ../../typescript@7.0.0/node_modules/typescript
      typescript@7.0.0
        node_modules
          typescript
      card@1.0.0(react@16.8.0(typescript@8.0.0))
        node_modules
          card
          react --> ../../react@16.8.0(typescript@8.0.0)/node_modules/react
      react@16.8.0(typescript@8.0.0)
        node_modules
          react
          typescript --> ../../typescript@8.0.0/node_modules/typescript
      typescript@8.0.0
        node_modules
          typescript
  ```

  In the above example, both projects have `card` in dependencies but the projects use different versions of `typescript`. Hence, even though the same version of `card` is used, `card` in `project1` will reference `react` from a directory where it is placed with `typescript@7.0.0` (because it resolves `typescript` from the dependencies of `project1`), while `card` in `project2` will reference `react` with `typescript@8.0.0`.

  Related issue: [#7444](https://github.com/pnpm/pnpm/issues/7444).
  Related PR: [#7606](https://github.com/pnpm/pnpm/pull/7606).

### Minor Changes

- 7733f3a: Added support for registry-scoped SSL configurations (cert, key, and ca). Three new settings supported: `<registryURL>:certfile`, `<registryURL>:keyfile`, and `<registryURL>:ca`. For instance:

  ```
  //registry.mycomp.com/:certfile=server-cert.pem
  //registry.mycomp.com/:keyfile=server-key.pem
  //registry.mycomp.com/:cafile=client-cert.pem
  ```

  Related issue: [#7427](https://github.com/pnpm/pnpm/issues/7427).
  Related PR: [#7626](https://github.com/pnpm/pnpm/pull/7626).

- 086b69c: The checksum of the `.pnpmfile.cjs` is saved into the lockfile. If the pnpmfile gets modified, the lockfile is reanalyzed to apply the changes [#7662](https://github.com/pnpm/pnpm/pull/7662).
- 9f8948c: Add a new option autoInstallPeersFromHighestMatch that makes pnpm install the highest version satisfying one of the peer dependencies even if the peer dependency ranges don't overlap.
- 730929e: Add a field named `ignoredOptionalDependencies`. This is an array of strings. If an optional dependency has its name included in this array, it will be skipped.

### Patch Changes

- 977060f: Properly resolve peer dependencies of peer dependencies [#7444](https://github.com/pnpm/pnpm/issues/7444).
- f5eadba: Revert [#7583](https://github.com/pnpm/pnpm/pull/7583).
- 7edb917: Deleting a dependencies field via a `readPackage` hook should work [#7704](https://github.com/pnpm/pnpm/pull/7704).
- 732430a: `bundledDependencies` should never be added to the lockfile with `false` as the value [#7576](https://github.com/pnpm/pnpm/issues/7576).
- 22c7acc: Link globally the command of a package that has no name in `package.json` [#4761](https://github.com/pnpm/pnpm/issues/4761).
- Updated dependencies [7733f3a]
- Updated dependencies [3ded840]
- Updated dependencies [cdd8365]
- Updated dependencies [c692f80]
- Updated dependencies [89b396b]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [27a96a8]
- Updated dependencies [b13d2dc]
- Updated dependencies [730929e]
- Updated dependencies [8eddd21]
- Updated dependencies [98a1266]
  - @pnpm/types@10.0.0
  - @pnpm/error@6.0.0
  - @pnpm/dependency-path@3.0.0
  - @pnpm/lockfile-utils@10.0.0
  - @pnpm/npm-resolver@19.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/pick-registry-for-package@6.0.0
  - @pnpm/which-version-is-pinned@6.0.0
  - @pnpm/read-package-json@9.0.0
  - @pnpm/store-controller-types@18.0.0
  - @pnpm/manifest-utils@6.0.0
  - @pnpm/lockfile-types@6.0.0
  - @pnpm/prune-lockfile@6.0.0
  - @pnpm/resolver-base@12.0.0
  - @pnpm/pick-fetcher@3.0.0
  - @pnpm/core-loggers@10.0.0
  - @pnpm/lockfile.preferred-versions@1.0.0

## 31.4.0

### Minor Changes

- 31054a63e: Running `pnpm update -r --latest` will no longer downgrade prerelease dependencies [#7436](https://github.com/pnpm/pnpm/issues/7436).

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/store-controller-types@17.2.0
  - @pnpm/resolver-base@11.1.0
  - @pnpm/npm-resolver@18.1.0
  - @pnpm/pick-fetcher@2.0.1
  - @pnpm/lockfile-utils@9.0.5

## 31.3.1

### Patch Changes

- 33313d2fd: Update rename-overwrite to v5.
- 4d34684f1: Added support for boolean values in 'bundleDependencies' package.json fields when installing a dependency. Fix to properly handle 'bundledDependencies' alias [#7411](https://github.com/pnpm/pnpm/issues/7411).
- Updated dependencies [33313d2fd]
- Updated dependencies [4d34684f1]
  - @pnpm/npm-resolver@18.0.2
  - @pnpm/lockfile-types@5.1.5
  - @pnpm/types@9.4.2
  - @pnpm/lockfile-utils@9.0.4
  - @pnpm/prune-lockfile@5.0.9
  - @pnpm/pick-registry-for-package@5.0.6
  - @pnpm/core-loggers@9.0.6
  - @pnpm/dependency-path@2.1.7
  - @pnpm/manifest-utils@5.0.7
  - @pnpm/read-package-json@8.0.7
  - @pnpm/resolver-base@11.0.2
  - @pnpm/store-controller-types@17.1.4
  - @pnpm/pick-fetcher@2.0.1

## 31.3.0

### Minor Changes

- 672c559e4: A new setting added for symlinking [injected dependencies](https://pnpm.io/package_json#dependenciesmetainjected) from the workspace, if their dependencies use the same peer dependencies as the dependent package. The setting is called `dedupe-injected-deps` [#7416](https://github.com/pnpm/pnpm/pull/7416).

### Patch Changes

- Added support for boolean values in 'bundleDependencies' package.json fields when installing a dependency. Fix to properly handle 'bundledDependencies' alias [#7411](https://github.com/pnpm/pnpm/issues/7411).
- Updated dependencies
  - @pnpm/lockfile-types@5.1.4
  - @pnpm/types@9.4.1
  - @pnpm/lockfile-utils@9.0.3
  - @pnpm/prune-lockfile@5.0.8
  - @pnpm/pick-registry-for-package@5.0.5
  - @pnpm/core-loggers@9.0.5
  - @pnpm/dependency-path@2.1.6
  - @pnpm/manifest-utils@5.0.6
  - @pnpm/read-package-json@8.0.6
  - @pnpm/npm-resolver@18.0.1
  - @pnpm/resolver-base@11.0.1
  - @pnpm/store-controller-types@17.1.3
  - @pnpm/pick-fetcher@2.0.1

## 31.2.7

### Patch Changes

- d5a176af7: Fix a bug where `--fix-lockfile` crashes on tarballs [#7368](https://github.com/pnpm/pnpm/issues/7368).
- Updated dependencies [d5a176af7]
  - @pnpm/lockfile-utils@9.0.2

## 31.2.6

### Patch Changes

- 5462cb6d4: Fix dependencies deduplication.

## 31.2.5

### Patch Changes

- 6558d1865: When `dedupe-direct-deps` is set to `true`, commands of dependencies should be deduplicated [#7359](https://github.com/pnpm/pnpm/pull/7359).

## 31.2.4

### Patch Changes

- Updated dependencies [cd4fcfff0]
  - @pnpm/npm-resolver@18.0.0

## 31.2.3

### Patch Changes

- Updated dependencies [b4194fe52]
  - @pnpm/lockfile-utils@9.0.1

## 31.2.2

### Patch Changes

- 4da7b463f: (Important) Increased the default amount of allowed concurrent network request on systems that have more than 16 CPUs [#7285](https://github.com/pnpm/pnpm/pull/7285).
- Updated dependencies [291607c5a]
  - @pnpm/store-controller-types@17.1.2
  - @pnpm/npm-resolver@17.0.0

## 31.2.1

### Patch Changes

- 7ea45afbe: If a package's tarball cannot be fetched, print the dependency chain that leads to the failed package [#7265](https://github.com/pnpm/pnpm/pull/7265).
- Updated dependencies [4c2450208]
- Updated dependencies [7ea45afbe]
- Updated dependencies [cfc017ee3]
  - @pnpm/lockfile-utils@9.0.0
  - @pnpm/resolver-base@11.0.0
  - @pnpm/npm-resolver@17.0.0
  - @pnpm/store-controller-types@17.1.1
  - @pnpm/exec.files-include-install-scripts@1.0.0
  - @pnpm/pick-fetcher@2.0.1

## 31.2.0

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

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/store-controller-types@17.1.0
  - @pnpm/types@9.4.0
  - @pnpm/pick-registry-for-package@5.0.4
  - @pnpm/lockfile-types@5.1.3
  - @pnpm/lockfile-utils@8.0.7
  - @pnpm/prune-lockfile@5.0.7
  - @pnpm/core-loggers@9.0.4
  - @pnpm/dependency-path@2.1.5
  - @pnpm/manifest-utils@5.0.5
  - @pnpm/read-package-json@8.0.5
  - @pnpm/npm-resolver@16.0.13
  - @pnpm/resolver-base@10.0.4
  - @pnpm/pick-fetcher@2.0.1

## 31.1.21

### Patch Changes

- Updated dependencies [01bc58e2c]
- Updated dependencies [ff55119a8]
  - @pnpm/npm-resolver@16.0.12

## 31.1.20

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/pick-registry-for-package@5.0.3
  - @pnpm/lockfile-types@5.1.2
  - @pnpm/lockfile-utils@8.0.6
  - @pnpm/prune-lockfile@5.0.6
  - @pnpm/core-loggers@9.0.3
  - @pnpm/dependency-path@2.1.4
  - @pnpm/manifest-utils@5.0.4
  - @pnpm/read-package-json@8.0.4
  - @pnpm/npm-resolver@16.0.11
  - @pnpm/resolver-base@10.0.3
  - @pnpm/store-controller-types@17.0.1
  - @pnpm/pick-fetcher@2.0.1

## 31.1.19

### Patch Changes

- b0afd7833: Optimize peers resolution to avoid out-of-memory exceptions in some rare cases, when there are too many circular dependencies and peer dependencies [#7149](https://github.com/pnpm/pnpm/pull/7149).

## 31.1.18

### Patch Changes

- f394cfccd: Don't update git-hosted dependencies when adding an unrelated dependency [#7008](https://github.com/pnpm/pnpm/issues/7008).
- Updated dependencies [f394cfccd]
  - @pnpm/lockfile-utils@8.0.5
  - @pnpm/pick-fetcher@2.0.1

## 31.1.17

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/store-controller-types@17.0.0
  - @pnpm/npm-resolver@16.0.10

## 31.1.16

### Patch Changes

- Updated dependencies [03cdccc6e]
  - @pnpm/store-controller-types@16.1.0
  - @pnpm/npm-resolver@16.0.9

## 31.1.15

### Patch Changes

- @pnpm/store-controller-types@16.0.1
- @pnpm/npm-resolver@16.0.9

## 31.1.14

### Patch Changes

- 77e24d341: Dedupe deps with the same alias in direct dependencies [6966](https://github.com/pnpm/pnpm/issues/6966)
- Updated dependencies [41c2b65cf]
- Updated dependencies [494f87544]
- Updated dependencies [e9aa6f682]
  - @pnpm/npm-resolver@16.0.9
  - @pnpm/store-controller-types@16.0.0
  - @pnpm/lockfile-utils@8.0.4

## 31.1.13

### Patch Changes

- a13a0e8f5: Installation succeeds if a non-optional dependency of an optional dependency has failing installation scripts [#6822](https://github.com/pnpm/pnpm/issues/6822).
  - @pnpm/npm-resolver@16.0.8

## 31.1.12

### Patch Changes

- Updated dependencies [aa2ae8fe2]
- Updated dependencies [e958707b2]
  - @pnpm/types@9.2.0
  - @pnpm/npm-resolver@16.0.8
  - @pnpm/pick-registry-for-package@5.0.2
  - @pnpm/lockfile-types@5.1.1
  - @pnpm/lockfile-utils@8.0.3
  - @pnpm/prune-lockfile@5.0.5
  - @pnpm/core-loggers@9.0.2
  - @pnpm/dependency-path@2.1.3
  - @pnpm/manifest-utils@5.0.3
  - @pnpm/read-package-json@8.0.3
  - @pnpm/resolver-base@10.0.2
  - @pnpm/store-controller-types@15.0.2

## 31.1.11

### Patch Changes

- e9684b559: replacing object copying with a prototype chain, avoiding extra memory allocations in resolveDependencies function
- 9b5110810: Replacing usages of ramda isEmpty, which happens to be slow and resource intensive
- 8a68f5ad2: replacing object copying with a prototype chain, avoiding extra memory allocations in resolvePeersOfNode function
- fee263822: Refactor resolve-dependencies to use maps and sets instead of objects
- 17e4a3ab1: Replacing object spread with a prototype chain, avoiding extra memory allocations in resolveDependenciesOfImporters.
- abdb77f48: Fix edge case where invalid "nodeId" was created. Small optimization.
- ba9335601: Prefer versions found in parent package dependencies only [#6737](https://github.com/pnpm/pnpm/issues/6737).
  - @pnpm/npm-resolver@16.0.7

## 31.1.10

### Patch Changes

- e2c3ef313: In cases where both aliased and non-aliased dependencies exist to the same package, non-aliased dependencies will be used for resolving peer dependencies, addressing issue [#6588](https://github.com/pnpm/pnpm/issues/6588).
- df3eb8313: Return bundled manifest for local dependency.

## 31.1.9

### Patch Changes

- 61f22f9ef: Don't add the version of a local directory dependency to the lockfile. This information is not used anywhere by pnpm and is only causing more Git conflicts [#6695](https://github.com/pnpm/pnpm/pull/6695).

## 31.1.8

### Patch Changes

- Updated dependencies [d9da627cd]
- Updated dependencies [302ebffc5]
  - @pnpm/lockfile-utils@8.0.2
  - @pnpm/constants@7.1.1
  - @pnpm/prune-lockfile@5.0.4
  - @pnpm/error@5.0.2
  - @pnpm/manifest-utils@5.0.2
  - @pnpm/read-package-json@8.0.2
  - @pnpm/npm-resolver@16.0.7

## 31.1.7

### Patch Changes

- e83eacdcc: When `dedupe-peer-dependents` is enabled (default), use the path (not id) to
  determine compatibility.

  When multiple dependency groups can be deduplicated, the
  latter ones are sorted according to number of peers to allow them to
  benefit from deduplication.

  Resolves: [#6605](https://github.com/pnpm/pnpm/issues/6605)

- 4b97f1f07: Don't use await in loops.
- d55b41a8b: Dependencies have been updated.
- Updated dependencies [d55b41a8b]
  - @pnpm/npm-resolver@16.0.6

## 31.1.6

### Patch Changes

- Updated dependencies [4fc497882]
- Updated dependencies [e6052260c]
- Updated dependencies [9c4ae87bd]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [9c4ae87bd]
  - @pnpm/which-version-is-pinned@5.0.1
  - @pnpm/npm-resolver@16.0.5
  - @pnpm/lockfile-types@5.1.0
  - @pnpm/types@9.1.0
  - @pnpm/manifest-utils@5.0.1
  - @pnpm/constants@7.1.0
  - @pnpm/lockfile-utils@8.0.1
  - @pnpm/prune-lockfile@5.0.3
  - @pnpm/pick-registry-for-package@5.0.1
  - @pnpm/core-loggers@9.0.1
  - @pnpm/dependency-path@2.1.2
  - @pnpm/read-package-json@8.0.1
  - @pnpm/resolver-base@10.0.1
  - @pnpm/store-controller-types@15.0.1
  - @pnpm/error@5.0.1

## 31.1.5

### Patch Changes

- ee78f144d: Peers resolution should not fail when a linked in dependency resolves a peer dependency.

## 31.1.4

### Patch Changes

- Updated dependencies [d58cdb962]
  - @pnpm/lockfile-utils@8.0.0

## 31.1.3

### Patch Changes

- d8c1013a9: Do not include external links in the lockfile, when they are used to resolve peers.

## 31.1.2

### Patch Changes

- Updated dependencies [edb3072a9]
  - @pnpm/npm-resolver@16.0.4

## 31.1.1

### Patch Changes

- c0760128d: bump semver to 7.4.0
- Updated dependencies [c0760128d]
  - @pnpm/dependency-path@2.1.1
  - @pnpm/npm-resolver@16.0.3
  - @pnpm/lockfile-utils@7.0.1
  - @pnpm/prune-lockfile@5.0.2

## 31.1.0

### Minor Changes

- 72ba638e3: When `excludeLinksFromLockfile` is set to `true`, linked dependencies are not added to the lockfile.

### Patch Changes

- e440d784f: Update yarn dependencies.
- d52c6d751: Don't print an info message about linked dependencies if they are real linked dependencies specified via the `link:` protocol in `package.json`.
- Updated dependencies [72ba638e3]
  - @pnpm/lockfile-utils@7.0.0

## 31.0.3

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [ef6c22e12]
- Updated dependencies [94f94eed6]
  - @pnpm/dependency-path@2.1.0
  - @pnpm/npm-resolver@16.0.2
  - @pnpm/lockfile-utils@6.0.1
  - @pnpm/prune-lockfile@5.0.1

## 31.0.2

### Patch Changes

- Updated dependencies [642f8c1d0]
  - @pnpm/npm-resolver@16.0.1

## 31.0.1

### Patch Changes

- 65e3af8a0: Remove the replaceall polyfill from the dependencies.

## 31.0.0

### Major Changes

- 1d105e7fc: Save the whole tarball URL in the lockfile, if it doesn't use the standard format [#6265](https://github.com/pnpm/pnpm/pull/6265).
- c92936158: The registry field is removed from the `resolution` object in `pnpm-lock.yaml`.
- 158d8cf22: `useLockfileV6` field is deleted. Lockfile v5 cannot be written anymore, only transformed to the new format.
- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- 634d6874b: Peer dependency is not unlinked when adding a new dependency [#6272](https://github.com/pnpm/pnpm/issues/6272).
- b4f26e41a: Fix regression introduced in v7.30.1 [#6271](https://github.com/pnpm/pnpm/issues/6271).
- cfb6bb3bf: Aliased packages should be used to resolve peer dependencies too [#4301](https://github.com/pnpm/pnpm/issues/4301).
- Updated dependencies [c92936158]
- Updated dependencies [ca8f51e60]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [f835994ea]
- Updated dependencies [0e26acb0f]
- Updated dependencies [9d026b7cb]
  - @pnpm/lockfile-types@5.0.0
  - @pnpm/lockfile-utils@6.0.0
  - @pnpm/dependency-path@2.0.0
  - @pnpm/pick-registry-for-package@5.0.0
  - @pnpm/which-version-is-pinned@5.0.0
  - @pnpm/read-package-json@8.0.0
  - @pnpm/store-controller-types@15.0.0
  - @pnpm/manifest-utils@5.0.0
  - @pnpm/prune-lockfile@5.0.0
  - @pnpm/resolver-base@10.0.0
  - @pnpm/npm-resolver@16.0.0
  - @pnpm/core-loggers@9.0.0
  - @pnpm/constants@7.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0

## 30.0.2

### Patch Changes

- @pnpm/npm-resolver@15.0.9

## 30.0.1

### Patch Changes

- 9d906fc94: Fix the incorrect error block when subproject has been patched [#6183](https://github.com/pnpm/pnpm/issues/6183)

## 30.0.0

### Major Changes

- 670bea844: The update options are passed on per project basis. So the `update` and `updateMatching` options are options of importers/projects.

## 29.4.0

### Minor Changes

- 5c31fa8be: A new setting is now supported: `dedupe-peer-dependents`.

  When this setting is set to `true`, packages with peer dependencies will be deduplicated after peers resolution.

  For instance, let's say we have a workspace with two projects and both of them have `webpack` in their dependencies. `webpack` has `esbuild` in its optional peer dependencies, and one of the projects has `esbuild` in its dependencies. In this case, pnpm will link two instances of `webpack` to the `node_modules/.pnpm` directory: one with `esbuild` and another one without it:

  ```
  node_modules
    .pnpm
      webpack@1.0.0_esbuild@1.0.0
      webpack@1.0.0
  project1
    node_modules
      webpack -> ../../node_modules/.pnpm/webpack@1.0.0/node_modules/webpack
  project2
    node_modules
      webpack -> ../../node_modules/.pnpm/webpack@1.0.0_esbuild@1.0.0/node_modules/webpack
      esbuild
  ```

  This makes sense because `webpack` is used in two projects, and one of the projects doesn't have `esbuild`, so the two projects cannot share the same instance of `webpack`. However, this is not what most developers expect, especially since in a hoisted `node_modules`, there would only be one instance of `webpack`. Therefore, you may now use the `dedupe-peer-dependents` setting to deduplicate `webpack` when it has no conflicting peer dependencies. In this case, if we set `dedupe-peer-dependents` to `true`, both projects will use the same `webpack` instance, which is the one that has `esbuild` resolved:

  ```
  node_modules
    .pnpm
      webpack@1.0.0_esbuild@1.0.0
  project1
    node_modules
      webpack -> ../../node_modules/.pnpm/webpack@1.0.0_esbuild@1.0.0/node_modules/webpack
  project2
    node_modules
      webpack -> ../../node_modules/.pnpm/webpack@1.0.0_esbuild@1.0.0/node_modules/webpack
      esbuild
  ```

## 29.3.2

### Patch Changes

- 1b2e09ccf: Fix a case of installs not being deterministic and causing lockfile changes between repeat installs. When a dependency only declares `peerDependenciesMeta` and not `peerDependencies`, `dependencies`, or `optionalDependencies`, the dependency's peers were not considered deterministically before.

## 29.3.1

### Patch Changes

- 029143cff: When resolving dependencies, prefer versions that are already used in the root of the project. This is important to minimize the number of packages that will be nested during hoisting [#6054](https://github.com/pnpm/pnpm/pull/6054).
- Updated dependencies [029143cff]
- Updated dependencies [029143cff]
  - @pnpm/resolver-base@9.2.0
  - @pnpm/npm-resolver@15.0.8
  - @pnpm/lockfile-utils@5.0.7
  - @pnpm/store-controller-types@14.3.1

## 29.3.0

### Minor Changes

- 59ee53678: A new `resolution-mode` added: `lowest-direct`. With this resolution mode direct dependencies will be resolved to their lowest versions. So if there is `foo@^1.1.0` in the dependencies, then `1.1.0` will be installed, even if the latest version of `foo` is `1.2.0`.

### Patch Changes

- Updated dependencies [d89d7a078]
- Updated dependencies [74b535f19]
- Updated dependencies [65563ae09]
  - @pnpm/dependency-path@1.1.3
  - @pnpm/npm-resolver@15.0.7
  - @pnpm/lockfile-utils@5.0.6
  - @pnpm/prune-lockfile@4.0.24

## 29.2.5

### Patch Changes

- 6348f5931: The update command should not replace dependency versions specified via dist-tags [#5996](https://github.com/pnpm/pnpm/pull/5996).
- Updated dependencies [9247f6781]
  - @pnpm/dependency-path@1.1.2
  - @pnpm/lockfile-utils@5.0.5
  - @pnpm/prune-lockfile@4.0.23

## 29.2.4

### Patch Changes

- 5cfe9e77a: Fix lockfile v6 on projects that use patched dependencies [#5967](https://github.com/pnpm/pnpm/issues/5967).

## 29.2.3

### Patch Changes

- 6c7ac6320: `pnpm install --fix-lockfile` should not fail if the package has no dependencies [#5878](https://github.com/pnpm/pnpm/issues/5878).
  - @pnpm/npm-resolver@15.0.6

## 29.2.2

### Patch Changes

- Updated dependencies [1e6de89b6]
  - @pnpm/npm-resolver@15.0.6

## 29.2.1

### Patch Changes

- Updated dependencies [0f6e95872]
  - @pnpm/dependency-path@1.1.1
  - @pnpm/lockfile-utils@5.0.4
  - @pnpm/prune-lockfile@4.0.22

## 29.2.0

### Minor Changes

- c7b05cd9a: When ignoreScripts=true is passed to the fetcher, do not build git-hosted dependencies.
- 3ebce5db7: Added support for `pnpm-lock.yaml` format v6. This new format will be the new lockfile format in pnpm v8. To use the new lockfile format, use the `use-lockfile-v6=true` setting in `.npmrc`. Or run `pnpm install --use-lockfile-v6` [#5810](https://github.com/pnpm/pnpm/pull/5810).

### Patch Changes

- Updated dependencies [891a8d763]
- Updated dependencies [c7b05cd9a]
- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
  - @pnpm/store-controller-types@14.3.0
  - @pnpm/constants@6.2.0
  - @pnpm/dependency-path@1.1.0
  - @pnpm/prune-lockfile@4.0.21
  - @pnpm/error@4.0.1
  - @pnpm/lockfile-utils@5.0.3
  - @pnpm/manifest-utils@4.1.4
  - @pnpm/read-package-json@7.0.5
  - @pnpm/npm-resolver@15.0.5

## 29.1.0

### Minor Changes

- 1fad508b0: When the `resolve-peers-from-workspace-root` setting is set to `true`, pnpm will use dependencies installed in the root of the workspace to resolve peer dependencies in any of the workspace's projects [#5882](https://github.com/pnpm/pnpm/pull/5882).

## 29.0.12

### Patch Changes

- Updated dependencies [83ba90fb8]
  - @pnpm/npm-resolver@15.0.4

## 29.0.11

### Patch Changes

- Updated dependencies [b77651d14]
- Updated dependencies [2458741fa]
  - @pnpm/types@8.10.0
  - @pnpm/store-controller-types@14.2.0
  - @pnpm/pick-registry-for-package@4.0.3
  - @pnpm/lockfile-types@4.3.6
  - @pnpm/lockfile-utils@5.0.2
  - @pnpm/prune-lockfile@4.0.20
  - @pnpm/core-loggers@8.0.3
  - @pnpm/dependency-path@1.0.1
  - @pnpm/manifest-utils@4.1.3
  - @pnpm/read-package-json@7.0.4
  - @pnpm/npm-resolver@15.0.3
  - @pnpm/resolver-base@9.1.5

## 29.0.10

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/dependency-path@1.0.0
  - @pnpm/lockfile-utils@5.0.1
  - @pnpm/prune-lockfile@4.0.19

## 29.0.9

### Patch Changes

- 49f6c917f: `pnpm update` should not replace `workspace:*`, `workspace:~`, and `workspace:^` with `workspace:<version>` [#5764](https://github.com/pnpm/pnpm/pull/5764).

## 29.0.8

### Patch Changes

- a9d59d8bc: Update dependencies.
- Updated dependencies [c245edf1b]
- Updated dependencies [a9d59d8bc]
- Updated dependencies [f3bfa2aae]
  - @pnpm/manifest-utils@4.1.2
  - @pnpm/read-package-json@7.0.3
  - @pnpm/npm-resolver@15.0.2

## 29.0.7

### Patch Changes

- Updated dependencies [ecc8794bb]
- Updated dependencies [ecc8794bb]
  - @pnpm/lockfile-utils@5.0.0

## 29.0.6

### Patch Changes

- 4a4b2ac93: Fix the nodeId in dependenciesTree for linked local packages.

## 29.0.5

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - @pnpm/core-loggers@8.0.2
  - dependency-path@9.2.8
  - @pnpm/lockfile-types@4.3.5
  - @pnpm/lockfile-utils@4.2.8
  - @pnpm/manifest-utils@4.1.1
  - @pnpm/npm-resolver@15.0.1
  - @pnpm/pick-registry-for-package@4.0.2
  - @pnpm/prune-lockfile@4.0.18
  - @pnpm/read-package-json@7.0.2
  - @pnpm/resolver-base@9.1.4
  - @pnpm/store-controller-types@14.1.5

## 29.0.4

### Patch Changes

- 0da2f0412: Update dependencies.

## 29.0.3

### Patch Changes

- 3c36e7e02: Don't crash on lockfile with no packages field [#5553](https://github.com/pnpm/pnpm/issues/5553).

## 29.0.2

### Patch Changes

- Updated dependencies [804de211e]
  - @pnpm/npm-resolver@15.0.0

## 29.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - @pnpm/manifest-utils@4.1.0
  - @pnpm/core-loggers@8.0.1
  - dependency-path@9.2.7
  - @pnpm/lockfile-types@4.3.4
  - @pnpm/lockfile-utils@4.2.7
  - @pnpm/npm-resolver@14.0.1
  - @pnpm/pick-registry-for-package@4.0.1
  - @pnpm/prune-lockfile@4.0.17
  - @pnpm/read-package-json@7.0.1
  - @pnpm/resolver-base@9.1.3
  - @pnpm/store-controller-types@14.1.4

## 29.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- e35988d1f: Update Yarn dependencies.
- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/core-loggers@8.0.0
  - @pnpm/manifest-utils@4.0.0
  - @pnpm/npm-resolver@14.0.0
  - @pnpm/pick-registry-for-package@4.0.0
  - @pnpm/read-package-json@7.0.0
  - @pnpm/which-version-is-pinned@4.0.0

## 28.4.5

### Patch Changes

- 84f440419: Don't crash when `auto-install-peers` is set to `true` and installation is done on a workspace with that has the same dependencies in multiple projects [#5454](https://github.com/pnpm/pnpm/issues/5454).
- Updated dependencies [3ae888c28]
  - @pnpm/core-loggers@7.1.0
  - @pnpm/manifest-utils@3.1.6
  - @pnpm/npm-resolver@13.1.11

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
