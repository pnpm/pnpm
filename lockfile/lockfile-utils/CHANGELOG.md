# @pnpm/lockfile-utils

## 9.0.5

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/resolver-base@11.1.0
  - @pnpm/pick-fetcher@2.0.1

## 9.0.4

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/lockfile-types@5.1.5
  - @pnpm/types@9.4.2
  - @pnpm/dependency-path@2.1.7
  - @pnpm/resolver-base@11.0.2
  - @pnpm/pick-fetcher@2.0.1

## 9.0.3

### Patch Changes

- Updated dependencies
  - @pnpm/lockfile-types@5.1.4
  - @pnpm/types@9.4.1
  - @pnpm/dependency-path@2.1.6
  - @pnpm/resolver-base@11.0.1
  - @pnpm/pick-fetcher@2.0.1

## 9.0.2

### Patch Changes

- d5a176af7: Fix a bug where `--fix-lockfile` crashes on tarballs [#7368](https://github.com/pnpm/pnpm/issues/7368).

## 9.0.1

### Patch Changes

- b4194fe52: Fixed out-of-memory exception that was happening on dependencies with many peer dependencies, when `node-linker` was set to `hoisted` [#6227](https://github.com/pnpm/pnpm/issues/6227).

## 9.0.0

### Major Changes

- 4c2450208: (Important) Tarball resolutions in `pnpm-lock.yaml` will no longer contain a `registry` field. This field has been unused for a long time. This change should not cause any issues besides backward compatible modifications to the lockfile [#7262](https://github.com/pnpm/pnpm/pull/7262).

### Patch Changes

- Updated dependencies [4c2450208]
  - @pnpm/resolver-base@11.0.0
  - @pnpm/pick-fetcher@2.0.1

## 8.0.7

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/lockfile-types@5.1.3
  - @pnpm/dependency-path@2.1.5
  - @pnpm/resolver-base@10.0.4
  - @pnpm/pick-fetcher@2.0.1

## 8.0.6

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/lockfile-types@5.1.2
  - @pnpm/dependency-path@2.1.4
  - @pnpm/resolver-base@10.0.3
  - @pnpm/pick-fetcher@2.0.1

## 8.0.5

### Patch Changes

- f394cfccd: Don't update git-hosted dependencies when adding an unrelated dependency [#7008](https://github.com/pnpm/pnpm/issues/7008).
- Updated dependencies [f394cfccd]
  - @pnpm/pick-fetcher@2.0.1

## 8.0.4

### Patch Changes

- e9aa6f682: Apply fixes from @typescript-eslint v6 for nullish coalescing and optional chains. No behavior changes are expected with this change.

## 8.0.3

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/lockfile-types@5.1.1
  - @pnpm/dependency-path@2.1.3
  - @pnpm/resolver-base@10.0.2

## 8.0.2

### Patch Changes

- d9da627cd: Should always treat local file dependency as new dependency [#5381](https://github.com/pnpm/pnpm/issues/5381)

## 8.0.1

### Patch Changes

- Updated dependencies [9c4ae87bd]
- Updated dependencies [a9e0b7cbf]
  - @pnpm/lockfile-types@5.1.0
  - @pnpm/types@9.1.0
  - @pnpm/dependency-path@2.1.2
  - @pnpm/resolver-base@10.0.1

## 8.0.0

### Major Changes

- d58cdb962: Return details about the reason why the lockfile doesn't satisfy the manifest.

## 7.0.1

### Patch Changes

- Updated dependencies [c0760128d]
  - @pnpm/dependency-path@2.1.1

## 7.0.0

### Major Changes

- 72ba638e3: Breaking changes to the API of `satisfiesPackageManifest`.

## 6.0.1

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
  - @pnpm/dependency-path@2.1.0

## 6.0.0

### Major Changes

- c92936158: The registry field is removed from the `resolution` object in `pnpm-lock.yaml`.
- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [c92936158]
- Updated dependencies [ca8f51e60]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [0e26acb0f]
  - @pnpm/lockfile-types@5.0.0
  - @pnpm/dependency-path@2.0.0
  - @pnpm/resolver-base@10.0.0
  - @pnpm/types@9.0.0

## 5.0.7

### Patch Changes

- Updated dependencies [029143cff]
- Updated dependencies [029143cff]
  - @pnpm/resolver-base@9.2.0

## 5.0.6

### Patch Changes

- Updated dependencies [d89d7a078]
  - @pnpm/dependency-path@1.1.3

## 5.0.5

### Patch Changes

- Updated dependencies [9247f6781]
  - @pnpm/dependency-path@1.1.2

## 5.0.4

### Patch Changes

- Updated dependencies [0f6e95872]
  - @pnpm/dependency-path@1.1.1

## 5.0.3

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/dependency-path@1.1.0

## 5.0.2

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/lockfile-types@4.3.6
  - @pnpm/dependency-path@1.0.1
  - @pnpm/resolver-base@9.1.5

## 5.0.1

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/dependency-path@1.0.0

## 5.0.0

### Major Changes

- ecc8794bb: Breaking change to the API of the `extendProjectsWithTargetDirs` function.

### Patch Changes

- ecc8794bb: Sync all injected dependencies when hoisted node linker is used.

## 4.2.8

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - dependency-path@9.2.8
  - @pnpm/lockfile-types@4.3.5
  - @pnpm/resolver-base@9.1.4

## 4.2.7

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - dependency-path@9.2.7
  - @pnpm/lockfile-types@4.3.4
  - @pnpm/resolver-base@9.1.3

## 4.2.6

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - dependency-path@9.2.6
  - @pnpm/lockfile-types@4.3.3
  - @pnpm/resolver-base@9.1.2

## 4.2.5

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - dependency-path@9.2.5
  - @pnpm/lockfile-types@4.3.2
  - @pnpm/resolver-base@9.1.1

## 4.2.4

### Patch Changes

- Updated dependencies [23984abd1]
  - @pnpm/resolver-base@9.1.0

## 4.2.3

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270

## 4.2.2

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - dependency-path@9.2.4
  - @pnpm/lockfile-types@4.3.1
  - @pnpm/resolver-base@9.0.6

## 4.2.1

### Patch Changes

- c83f40c10: pnpm should not consider a lockfile out-of-date if `auto-install-peers` is set to `true` and the peer dependency is in `devDependencies` or `optionalDependencies` [#5080](https://github.com/pnpm/pnpm/issues/5080).

## 4.2.0

### Minor Changes

- 8dcfbe357: Add `publishDirectory` field to the lockfile and relink the project when it changes.

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-types@4.3.0

## 4.1.0

### Minor Changes

- e3f4d131c: New option added: autoInstallPeers.

## 4.0.10

### Patch Changes

- dependency-path@9.2.3

## 4.0.9

### Patch Changes

- 5f643f23b: Update ramda to v0.28.

## 4.0.8

### Patch Changes

- Updated dependencies [fc581d371]
  - dependency-path@9.2.2

## 4.0.7

### Patch Changes

- Updated dependencies [d01c32355]
- Updated dependencies [8e5b77ef6]
- Updated dependencies [8e5b77ef6]
  - @pnpm/lockfile-types@4.2.0
  - @pnpm/types@8.4.0
  - dependency-path@9.2.1
  - @pnpm/resolver-base@9.0.5

## 4.0.6

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [c635f9fc1]
  - @pnpm/types@8.3.0
  - @pnpm/lockfile-types@4.1.0
  - dependency-path@9.2.0
  - @pnpm/resolver-base@9.0.4

## 4.0.5

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [725636a90]
  - @pnpm/types@8.2.0
  - dependency-path@9.1.4
  - @pnpm/lockfile-types@4.0.3
  - @pnpm/resolver-base@9.0.3

## 4.0.4

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - dependency-path@9.1.3
  - @pnpm/lockfile-types@4.0.2
  - @pnpm/resolver-base@9.0.2

## 4.0.3

### Patch Changes

- Updated dependencies [c57695550]
  - dependency-path@9.1.2

## 4.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - dependency-path@9.1.1
  - @pnpm/lockfile-types@4.0.1
  - @pnpm/resolver-base@9.0.1

## 4.0.1

### Patch Changes

- 688b0eaff: When checking if the lockfile is up-to-date, an empty `dependenciesMeta` field in the manifest should be satisfied by a not set field in the lockfile [#4463](https://github.com/pnpm/pnpm/pull/4463).
- Updated dependencies [0a70aedb1]
  - dependency-path@9.1.0

## 4.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [faf830b8f]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - dependency-path@9.0.0
  - @pnpm/lockfile-types@4.0.0
  - @pnpm/resolver-base@9.0.0

## 3.2.1

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-types@3.2.0
  - @pnpm/types@7.10.0
  - dependency-path@8.0.11
  - @pnpm/resolver-base@8.1.6

## 3.2.0

### Minor Changes

- cdc521cfa: Injected package location should be properly detected in a hoisted `node_modules`.

## 3.1.6

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - dependency-path@8.0.10
  - @pnpm/lockfile-types@3.1.5
  - @pnpm/resolver-base@8.1.5

## 3.1.5

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - dependency-path@8.0.9
  - @pnpm/lockfile-types@3.1.4
  - @pnpm/resolver-base@8.1.4

## 3.1.4

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - dependency-path@8.0.8
  - @pnpm/lockfile-types@3.1.3
  - @pnpm/resolver-base@8.1.3

## 3.1.3

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - dependency-path@8.0.7
  - @pnpm/lockfile-types@3.1.2
  - @pnpm/resolver-base@8.1.2

## 3.1.2

### Patch Changes

- 3cf543fc1: Non-standard tarball URL should be correctly calculated when the registry has no trailing slash in the configuration file [#4052](https://github.com/pnpm/pnpm/issues/4052). This is a regression caused introduced in v6.23.2 caused by [#4032](https://github.com/pnpm/pnpm/pull/4032).

## 3.1.1

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - dependency-path@8.0.6
  - @pnpm/lockfile-types@3.1.1
  - @pnpm/resolver-base@8.1.1

## 3.1.0

### Minor Changes

- 4ab87844a: New utility function added: `extendProjectsWithTargetDirs()`.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/resolver-base@8.1.0
  - @pnpm/lockfile-types@3.1.0
  - dependency-path@8.0.5

## 3.0.8

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - dependency-path@8.0.4
  - @pnpm/resolver-base@8.0.4

## 3.0.7

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - dependency-path@8.0.3
  - @pnpm/resolver-base@8.0.3

## 3.0.6

### Patch Changes

- Updated dependencies [6c418943c]
  - dependency-path@8.0.2

## 3.0.5

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - dependency-path@8.0.1
  - @pnpm/resolver-base@8.0.2

## 3.0.4

### Patch Changes

- a1a03d145: Import only the required functions from ramda.

## 3.0.3

### Patch Changes

- Updated dependencies [20e2f235d]
  - dependency-path@8.0.0

## 3.0.2

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - dependency-path@7.0.1
  - @pnpm/resolver-base@8.0.1

## 3.0.1

### Patch Changes

- Updated dependencies [9ceab68f0]
  - dependency-path@7.0.0

## 3.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [6871d74b2]
- Updated dependencies [e4efddbd2]
- Updated dependencies [f2bb5cbeb]
  - dependency-path@6.0.0
  - @pnpm/lockfile-types@3.0.0
  - @pnpm/resolver-base@8.0.0
  - @pnpm/types@7.0.0

## 2.0.22

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [9ad8c27bf]
  - @pnpm/lockfile-types@2.2.0
  - @pnpm/types@6.4.0
  - dependency-path@5.1.1
  - @pnpm/resolver-base@7.1.1

## 2.0.21

### Patch Changes

- Updated dependencies [e27dcf0dc]
  - dependency-path@5.1.0

## 2.0.20

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/resolver-base@7.1.0

## 2.0.19

### Patch Changes

- Updated dependencies [39142e2ad]
  - dependency-path@5.0.6

## 2.0.18

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/lockfile-types@2.1.1
  - @pnpm/types@6.3.1
  - dependency-path@5.0.5
  - @pnpm/resolver-base@7.0.5

## 2.0.17

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [d54043ee4]
  - @pnpm/lockfile-types@2.1.0
  - @pnpm/types@6.3.0
  - dependency-path@5.0.4
  - @pnpm/resolver-base@7.0.4

## 2.0.16

### Patch Changes

- 1140ef721: When getting resolution from package snapshot, always prefer the registry that is present in the package snapshot.
- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - dependency-path@5.0.3

## 2.0.15

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - dependency-path@5.0.2
  - @pnpm/resolver-base@7.0.3

## 2.0.14

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - dependency-path@5.0.1
  - @pnpm/resolver-base@7.0.2

## 2.0.13

### Patch Changes

- Updated dependencies [41d92948b]
  - dependency-path@5.0.0

## 2.0.12

### Patch Changes

- Updated dependencies [da091c711]
- Updated dependencies [6a8a97eee]
  - @pnpm/types@6.0.0
  - @pnpm/lockfile-types@2.0.1
  - dependency-path@4.0.7
  - @pnpm/resolver-base@7.0.1

## 2.0.12-alpha.1

### Patch Changes

- Updated dependencies [6a8a97eee]
  - @pnpm/lockfile-types@2.0.1-alpha.0

## 2.0.12-alpha.0

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0

## 2.0.11

### Patch Changes

- 907c63a48: Dependencies updated.
