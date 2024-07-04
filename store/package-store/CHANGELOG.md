# @pnpm/package-store

## 20.3.1

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/resolver-base@13.0.0
  - @pnpm/types@11.0.0
  - @pnpm/fetcher-base@16.0.3
  - @pnpm/package-requester@25.2.1
  - @pnpm/store-controller-types@18.1.2
  - @pnpm/store.cafs@3.0.4
  - @pnpm/worker@1.0.5
  - @pnpm/create-cafs-store@7.0.4

## 20.3.0

### Minor Changes

- 7c6c923: Some registries allow the exact same content to be published under different package names and/or versions. This breaks the validity checks of packages in the store. To avoid errors when verifying the names and versions of such packages in the store, you may now set the `strict-store-pkg-content-check` setting to `false` [#4724](https://github.com/pnpm/pnpm/issues/4724).

### Patch Changes

- Updated dependencies [7c6c923]
- Updated dependencies [13e55b2]
  - @pnpm/package-requester@25.2.0
  - @pnpm/types@10.1.1
  - @pnpm/fetcher-base@16.0.2
  - @pnpm/resolver-base@12.0.2
  - @pnpm/store.cafs@3.0.3
  - @pnpm/store-controller-types@18.1.1
  - @pnpm/worker@1.0.4
  - @pnpm/create-cafs-store@7.0.3

## 20.2.1

### Patch Changes

- @pnpm/package-requester@25.1.4
- @pnpm/create-cafs-store@7.0.2

## 20.2.0

### Minor Changes

- 0c08e1c: Added a new function for clearing resolution cache.

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/store-controller-types@18.1.0
  - @pnpm/package-requester@25.1.3
  - @pnpm/store.cafs@3.0.2
  - @pnpm/create-cafs-store@7.0.2
  - @pnpm/worker@1.0.3

## 20.1.2

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/fetcher-base@16.0.1
  - @pnpm/package-requester@25.1.2
  - @pnpm/resolver-base@12.0.1
  - @pnpm/store.cafs@3.0.1
  - @pnpm/store-controller-types@18.0.1
  - @pnpm/worker@1.0.2
  - @pnpm/create-cafs-store@7.0.1

## 20.1.1

### Patch Changes

- @pnpm/package-requester@25.1.1
- @pnpm/worker@1.0.1

## 20.1.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/package-requester@25.1.0
  - @pnpm/create-cafs-store@7.0.0

## 20.0.1

### Patch Changes

- Updated dependencies [b7d2ed4]
  - @pnpm/package-requester@25.0.1

## 20.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [3ded840]
- Updated dependencies [cdd8365]
- Updated dependencies [43cdd87]
- Updated dependencies [6cdbf11]
- Updated dependencies [36dcaa0]
- Updated dependencies [11d9ebd]
- Updated dependencies [36dcaa0]
- Updated dependencies [b13d2dc]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/worker@1.0.0
  - @pnpm/package-requester@25.0.0
  - @pnpm/store-controller-types@18.0.0
  - @pnpm/resolver-base@12.0.0
  - @pnpm/create-cafs-store@7.0.0
  - @pnpm/fetcher-base@16.0.0
  - @pnpm/store.cafs@3.0.0

## 19.0.15

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/package-requester@24.1.8
  - @pnpm/store-controller-types@17.2.0
  - @pnpm/resolver-base@11.1.0
  - @pnpm/store.cafs@2.0.12
  - @pnpm/create-cafs-store@6.0.13
  - @pnpm/fetcher-base@15.0.7
  - @pnpm/worker@0.3.14

## 19.0.14

### Patch Changes

- @pnpm/create-cafs-store@6.0.12
- @pnpm/package-requester@24.1.7
- @pnpm/worker@0.3.13

## 19.0.13

### Patch Changes

- Updated dependencies [33313d2fd]
- Updated dependencies [4d34684f1]
  - @pnpm/store.cafs@2.0.11
  - @pnpm/types@9.4.2
  - @pnpm/create-cafs-store@6.0.11
  - @pnpm/package-requester@24.1.6
  - @pnpm/worker@0.3.12
  - @pnpm/fetcher-base@15.0.6
  - @pnpm/resolver-base@11.0.2
  - @pnpm/store-controller-types@17.1.4

## 19.0.12

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/fetcher-base@15.0.5
  - @pnpm/package-requester@24.1.5
  - @pnpm/resolver-base@11.0.1
  - @pnpm/store.cafs@2.0.10
  - @pnpm/store-controller-types@17.1.3
  - @pnpm/worker@0.3.11
  - @pnpm/create-cafs-store@6.0.10

## 19.0.11

### Patch Changes

- @pnpm/create-cafs-store@6.0.9
- @pnpm/package-requester@24.1.4
- @pnpm/worker@0.3.10

## 19.0.10

### Patch Changes

- Updated dependencies [1e7bd4af3]
  - @pnpm/package-requester@24.1.3
  - @pnpm/worker@0.3.9

## 19.0.9

### Patch Changes

- 291607c5a: When using `pnpm store prune --force` alien directories are removed from the store [#7272](https://github.com/pnpm/pnpm/pull/7272).
- Updated dependencies [291607c5a]
  - @pnpm/store-controller-types@17.1.2
  - @pnpm/package-requester@24.1.2
  - @pnpm/store.cafs@2.0.9
  - @pnpm/create-cafs-store@6.0.8
  - @pnpm/worker@0.3.8

## 19.0.8

### Patch Changes

- Updated dependencies [4c2450208]
- Updated dependencies [cfc017ee3]
- Updated dependencies [7ea45afbe]
  - @pnpm/resolver-base@11.0.0
  - @pnpm/create-cafs-store@6.0.7
  - @pnpm/package-requester@24.1.1
  - @pnpm/store-controller-types@17.1.1
  - @pnpm/fetcher-base@15.0.4
  - @pnpm/worker@0.3.7
  - @pnpm/store.cafs@2.0.8

## 19.0.7

### Patch Changes

- Updated dependencies [43ce9e4a6]
- Updated dependencies [6390033cd]
  - @pnpm/package-requester@24.1.0
  - @pnpm/store-controller-types@17.1.0
  - @pnpm/types@9.4.0
  - @pnpm/worker@0.3.6
  - @pnpm/store.cafs@2.0.7
  - @pnpm/create-cafs-store@6.0.6
  - @pnpm/fetcher-base@15.0.3
  - @pnpm/resolver-base@10.0.4

## 19.0.6

### Patch Changes

- @pnpm/create-cafs-store@6.0.5
- @pnpm/package-requester@24.0.6
- @pnpm/worker@0.3.5

## 19.0.5

### Patch Changes

- 01bc58e2c: Update ssri to v10.0.5.
- Updated dependencies [01bc58e2c]
- Updated dependencies [08b65ff78]
  - @pnpm/package-requester@24.0.5
  - @pnpm/store.cafs@2.0.6
  - @pnpm/worker@0.3.4
  - @pnpm/create-cafs-store@6.0.4

## 19.0.4

### Patch Changes

- @pnpm/create-cafs-store@6.0.3
- @pnpm/package-requester@24.0.4
- @pnpm/worker@0.3.3

## 19.0.3

### Patch Changes

- @pnpm/create-cafs-store@6.0.2
- @pnpm/package-requester@24.0.3
- @pnpm/worker@0.3.2

## 19.0.2

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/create-cafs-store@6.0.1
  - @pnpm/fetcher-base@15.0.2
  - @pnpm/package-requester@24.0.2
  - @pnpm/resolver-base@10.0.3
  - @pnpm/store.cafs@2.0.5
  - @pnpm/store-controller-types@17.0.1
  - @pnpm/worker@0.3.1

## 19.0.1

### Patch Changes

- @pnpm/package-requester@24.0.1

## 19.0.0

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/store-controller-types@17.0.0
  - @pnpm/worker@0.3.0
  - @pnpm/package-requester@24.0.0
  - @pnpm/create-cafs-store@6.0.0
  - @pnpm/store.cafs@2.0.4
  - @pnpm/fetcher-base@15.0.1

## 18.0.1

### Patch Changes

- @pnpm/create-cafs-store@5.1.1
- @pnpm/package-requester@23.0.1
- @pnpm/worker@0.2.1

## 18.0.0

### Patch Changes

- Updated dependencies [03cdccc6e]
- Updated dependencies [48dcd108c]
  - @pnpm/store-controller-types@16.1.0
  - @pnpm/create-cafs-store@5.1.0
  - @pnpm/worker@0.2.0
  - @pnpm/package-requester@23.0.0
  - @pnpm/store.cafs@2.0.3
  - @pnpm/fetcher-base@15.0.1

## 17.0.2

### Patch Changes

- Updated dependencies [b3947185c]
  - @pnpm/store.cafs@2.0.2
  - @pnpm/package-requester@22.0.2
  - @pnpm/create-cafs-store@5.0.2
  - @pnpm/worker@0.1.2

## 17.0.1

### Patch Changes

- Updated dependencies [b548f2f43]
- Updated dependencies [4a1a9431d]
  - @pnpm/store.cafs@2.0.1
  - @pnpm/fetcher-base@15.0.1
  - @pnpm/package-requester@22.0.1
  - @pnpm/create-cafs-store@5.0.1
  - @pnpm/worker@0.1.1
  - @pnpm/store-controller-types@16.0.1

## 17.0.0

### Major Changes

- 494f87544: Breaking changes to the API.

### Patch Changes

- Updated dependencies [0fd9e6a6c]
- Updated dependencies [f2009d175]
- Updated dependencies [494f87544]
- Updated dependencies [70b2830ac]
- Updated dependencies [083bbf590]
- Updated dependencies [083bbf590]
- Updated dependencies [083bbf590]
  - @pnpm/store.cafs@2.0.0
  - @pnpm/create-cafs-store@5.0.0
  - @pnpm/package-requester@22.0.0
  - @pnpm/store-controller-types@16.0.0
  - @pnpm/fetcher-base@15.0.0
  - @pnpm/worker@0.1.0

## 16.1.0

### Minor Changes

- 92f42224c: New option added: `relinkLocalDirDeps`. It is `true` by default. When `false`, local directory dependencies are not relinked on repeat install.

### Patch Changes

- Updated dependencies [92f42224c]
  - @pnpm/package-requester@21.1.0

## 16.0.12

### Patch Changes

- Updated dependencies [dac59e632]
  - @pnpm/package-requester@21.0.12

## 16.0.11

### Patch Changes

- Updated dependencies [73f2b6826]
  - @pnpm/package-requester@21.0.11
  - @pnpm/store.cafs@1.0.2
  - @pnpm/create-cafs-store@4.0.8

## 16.0.10

### Patch Changes

- Updated dependencies [fe1c5f48d]
  - @pnpm/store.cafs@1.0.1
  - @pnpm/package-requester@21.0.10
  - @pnpm/create-cafs-store@4.0.7

## 16.0.9

### Patch Changes

- Updated dependencies [4bbf482d1]
  - @pnpm/store.cafs@1.0.0
  - @pnpm/package-requester@21.0.9
  - @pnpm/create-cafs-store@4.0.6

## 16.0.8

### Patch Changes

- Updated dependencies [aa2ae8fe2]
- Updated dependencies [250f7e9fe]
- Updated dependencies [e958707b2]
  - @pnpm/types@9.2.0
  - @pnpm/cafs@7.0.5
  - @pnpm/package-requester@21.0.8
  - @pnpm/fetcher-base@14.0.2
  - @pnpm/resolver-base@10.0.2
  - @pnpm/store-controller-types@15.0.2
  - @pnpm/create-cafs-store@4.0.5

## 16.0.7

### Patch Changes

- Updated dependencies [b81cefdcd]
  - @pnpm/cafs@7.0.4
  - @pnpm/package-requester@21.0.7
  - @pnpm/create-cafs-store@4.0.4

## 16.0.6

### Patch Changes

- Updated dependencies [e57e2d340]
  - @pnpm/cafs@7.0.3
  - @pnpm/package-requester@21.0.6
  - @pnpm/create-cafs-store@4.0.3

## 16.0.5

### Patch Changes

- @pnpm/package-requester@21.0.5
- @pnpm/create-cafs-store@4.0.2

## 16.0.4

### Patch Changes

- 4b97f1f07: Don't use await in loops.
- d55b41a8b: Dependencies have been updated.
- Updated dependencies [d55b41a8b]
- Updated dependencies [614d5bd72]
  - @pnpm/package-requester@21.0.4
  - @pnpm/cafs@7.0.2
  - @pnpm/create-cafs-store@4.0.2

## 16.0.3

### Patch Changes

- Updated dependencies [a9e0b7cbf]
  - @pnpm/types@9.1.0
  - @pnpm/fetcher-base@14.0.1
  - @pnpm/package-requester@21.0.3
  - @pnpm/resolver-base@10.0.1
  - @pnpm/cafs@7.0.1
  - @pnpm/store-controller-types@15.0.1
  - @pnpm/create-cafs-store@4.0.1

## 16.0.2

### Patch Changes

- Updated dependencies [c0760128d]
  - @pnpm/package-requester@21.0.2

## 16.0.1

### Patch Changes

- @pnpm/package-requester@21.0.1

## 16.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/package-requester@21.0.0
  - @pnpm/store-controller-types@15.0.0
  - @pnpm/resolver-base@10.0.0
  - @pnpm/create-cafs-store@4.0.0
  - @pnpm/fetcher-base@14.0.0
  - @pnpm/types@9.0.0
  - @pnpm/cafs@7.0.0

## 15.1.8

### Patch Changes

- @pnpm/package-requester@20.1.7
- @pnpm/cafs@6.0.2
- @pnpm/create-cafs-store@3.1.6

## 15.1.7

### Patch Changes

- Updated dependencies [029143cff]
- Updated dependencies [029143cff]
  - @pnpm/resolver-base@9.2.0
  - @pnpm/fetcher-base@13.1.6
  - @pnpm/package-requester@20.1.6
  - @pnpm/store-controller-types@14.3.1
  - @pnpm/create-cafs-store@3.1.5
  - @pnpm/cafs@6.0.1

## 15.1.6

### Patch Changes

- @pnpm/package-requester@20.1.5

## 15.1.5

### Patch Changes

- @pnpm/package-requester@20.1.4

## 15.1.4

### Patch Changes

- @pnpm/create-cafs-store@3.1.4
- @pnpm/package-requester@20.1.3

## 15.1.3

### Patch Changes

- Updated dependencies [98d6603f3]
- Updated dependencies [98d6603f3]
  - @pnpm/cafs@6.0.0
  - @pnpm/package-requester@20.1.3
  - @pnpm/create-cafs-store@3.1.3

## 15.1.2

### Patch Changes

- 1e6de89b6: Update ssri to v10.0.1.
- Updated dependencies [1e6de89b6]
  - @pnpm/package-requester@20.1.2
  - @pnpm/cafs@5.0.6
  - @pnpm/create-cafs-store@3.1.2

## 15.1.1

### Patch Changes

- @pnpm/package-requester@20.1.1

## 15.1.0

### Minor Changes

- 891a8d763: New function added to the store: `getFilesIndexFilePath`.

### Patch Changes

- Updated dependencies [891a8d763]
- Updated dependencies [c7b05cd9a]
  - @pnpm/package-requester@20.1.0
  - @pnpm/store-controller-types@14.3.0
  - @pnpm/cafs@5.0.5
  - @pnpm/create-cafs-store@3.1.1

## 15.0.5

### Patch Changes

- Updated dependencies [b77651d14]
- Updated dependencies [2458741fa]
  - @pnpm/types@8.10.0
  - @pnpm/create-cafs-store@3.1.0
  - @pnpm/store-controller-types@14.2.0
  - @pnpm/fetcher-base@13.1.5
  - @pnpm/package-requester@20.0.5
  - @pnpm/resolver-base@9.1.5
  - @pnpm/cafs@5.0.4

## 15.0.4

### Patch Changes

- @pnpm/package-requester@20.0.4

## 15.0.3

### Patch Changes

- a9d59d8bc: Update dependencies.
- Updated dependencies [a9d59d8bc]
  - @pnpm/package-requester@20.0.3
  - @pnpm/cafs@5.0.3
  - @pnpm/create-cafs-store@3.0.3

## 15.0.2

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - @pnpm/cafs@5.0.2
  - @pnpm/fetcher-base@13.1.4
  - @pnpm/package-requester@20.0.2
  - @pnpm/resolver-base@9.1.4
  - @pnpm/store-controller-types@14.1.5
  - @pnpm/create-cafs-store@3.0.2

## 15.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - @pnpm/cafs@5.0.1
  - @pnpm/fetcher-base@13.1.3
  - @pnpm/package-requester@20.0.1
  - @pnpm/resolver-base@9.1.3
  - @pnpm/store-controller-types@14.1.4
  - @pnpm/create-cafs-store@3.0.1

## 15.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/cafs@5.0.0
  - @pnpm/create-cafs-store@3.0.0
  - @pnpm/package-requester@20.0.0

## 14.2.7

### Patch Changes

- @pnpm/package-requester@19.0.6
- @pnpm/create-cafs-store@2.2.5

## 14.2.6

### Patch Changes

- @pnpm/package-requester@19.0.5

## 14.2.5

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/cafs@4.3.2
  - @pnpm/fetcher-base@13.1.2
  - @pnpm/package-requester@19.0.4
  - @pnpm/resolver-base@9.1.2
  - @pnpm/store-controller-types@14.1.3
  - @pnpm/create-cafs-store@2.2.4

## 14.2.4

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/cafs@4.3.1
  - @pnpm/fetcher-base@13.1.1
  - @pnpm/package-requester@19.0.3
  - @pnpm/resolver-base@9.1.1
  - @pnpm/store-controller-types@14.1.2
  - @pnpm/create-cafs-store@2.2.3

## 14.2.3

### Patch Changes

- 17e69e18b: `pnpm store prune` should remove all cached metadata.
  - @pnpm/package-requester@19.0.2

## 14.2.2

### Patch Changes

- Updated dependencies [745143e79]
  - @pnpm/cafs@4.3.0
  - @pnpm/create-cafs-store@2.2.2
  - @pnpm/package-requester@19.0.2
  - @pnpm/fetcher-base@13.1.0
  - @pnpm/store-controller-types@14.1.1

## 14.2.1

### Patch Changes

- dbac0ca01: Update ssri to v9.
- Updated dependencies [dbac0ca01]
  - @pnpm/cafs@4.2.1
  - @pnpm/package-requester@19.0.1
  - @pnpm/create-cafs-store@2.2.1

## 14.2.0

### Minor Changes

- 23984abd1: Add hook for adding custom fetchers.

### Patch Changes

- Updated dependencies [32915f0e4]
- Updated dependencies [23984abd1]
- Updated dependencies [7a17f99ab]
  - @pnpm/cafs@4.2.0
  - @pnpm/create-cafs-store@2.2.0
  - @pnpm/fetcher-base@13.1.0
  - @pnpm/package-requester@19.0.0
  - @pnpm/store-controller-types@14.1.1
  - @pnpm/resolver-base@9.1.0

## 14.1.1

### Patch Changes

- Updated dependencies [c191ca7bf]
  - @pnpm/cafs@4.1.0
  - @pnpm/package-requester@18.1.3
  - @pnpm/create-cafs-store@2.1.1

## 14.1.0

### Minor Changes

- 65c4260de: Support a new hook for passing a custom package importer to the store controller.

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [39c040127]
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
  - @pnpm/cafs@4.0.9
  - @pnpm/package-requester@18.1.2
  - @pnpm/create-cafs-store@2.1.0
  - @pnpm/store-controller-types@14.1.0

## 14.0.7

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/cafs@4.0.8
  - @pnpm/fetcher-base@13.0.2
  - @pnpm/package-requester@18.1.1
  - @pnpm/resolver-base@9.0.6
  - @pnpm/store-controller-types@14.0.2
  - @pnpm/create-cafs-store@2.0.3

## 14.0.6

### Patch Changes

- Updated dependencies [cac34ad69]
  - @pnpm/package-requester@18.1.0

## 14.0.5

### Patch Changes

- @pnpm/package-requester@18.0.13

## 14.0.4

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
  - @pnpm/package-requester@18.0.12

## 14.0.3

### Patch Changes

- @pnpm/package-requester@18.0.11

## 14.0.2

### Patch Changes

- @pnpm/create-cafs-store@2.0.2
- @pnpm/package-requester@18.0.10

## 14.0.1

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/cafs@4.0.7
  - @pnpm/fetcher-base@13.0.1
  - @pnpm/package-requester@18.0.10
  - @pnpm/resolver-base@9.0.5
  - @pnpm/store-controller-types@14.0.1
  - @pnpm/create-cafs-store@2.0.1

## 14.0.0

### Major Changes

- 2a34b21ce: Rename engine and targetEngine fields to sideEffectsCacheKey.

### Minor Changes

- 47b5e45dd: `package-import-method` supports a new option: `clone-or-copy`.

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/types@8.3.0
  - @pnpm/create-cafs-store@2.0.0
  - @pnpm/fetcher-base@13.0.0
  - @pnpm/store-controller-types@14.0.0
  - @pnpm/cafs@4.0.6
  - @pnpm/package-requester@18.0.9
  - @pnpm/resolver-base@9.0.4

## 13.0.8

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [0abfe1718]
- Updated dependencies [0abfe1718]
- Updated dependencies [0abfe1718]
  - @pnpm/types@8.2.0
  - @pnpm/package-requester@18.0.8
  - @pnpm/create-cafs-store@1.1.0
  - @pnpm/fetcher-base@12.1.0
  - @pnpm/cafs@4.0.5
  - @pnpm/resolver-base@9.0.3
  - @pnpm/store-controller-types@13.0.4

## 13.0.7

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/cafs@4.0.4
  - @pnpm/fetcher-base@12.0.3
  - @pnpm/package-requester@18.0.7
  - @pnpm/resolver-base@9.0.2
  - @pnpm/store-controller-types@13.0.3
  - @pnpm/create-cafs-store@1.0.3

## 13.0.6

### Patch Changes

- Updated dependencies [6756c2b02]
  - @pnpm/cafs@4.0.3
  - @pnpm/fetcher-base@12.0.2
  - @pnpm/package-requester@18.0.6
  - @pnpm/store-controller-types@13.0.2
  - @pnpm/create-cafs-store@1.0.2

## 13.0.5

### Patch Changes

- @pnpm/package-requester@18.0.5

## 13.0.4

### Patch Changes

- @pnpm/package-requester@18.0.4

## 13.0.3

### Patch Changes

- Updated dependencies [cadefe5b6]
  - @pnpm/cafs@4.0.2
  - @pnpm/create-cafs-store@1.0.1
  - @pnpm/package-requester@18.0.3

## 13.0.2

### Patch Changes

- Updated dependencies [1ceb632b1]
- Updated dependencies [18ba5e2c0]
  - @pnpm/create-cafs-store@1.0.0
  - @pnpm/types@8.0.1
  - @pnpm/package-requester@18.0.2
  - @pnpm/fetcher-base@12.0.1
  - @pnpm/resolver-base@9.0.1
  - @pnpm/store-controller-types@13.0.1
  - @pnpm/cafs@4.0.1

## 13.0.1

### Patch Changes

- Updated dependencies [7cdca5ef2]
  - @pnpm/package-requester@18.0.1

## 13.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [9c22c063e]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/package-requester@18.0.0
  - @pnpm/cafs@4.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/fetcher-base@12.0.0
  - @pnpm/resolver-base@9.0.0
  - @pnpm/store-controller-types@13.0.0

## 12.1.12

### Patch Changes

- Updated dependencies [5c525db13]
  - @pnpm/package-requester@17.0.0
  - @pnpm/store-controller-types@12.0.0
  - @pnpm/cafs@3.0.15

## 12.1.11

### Patch Changes

- Updated dependencies [800fb2836]
- Updated dependencies [b138d048c]
  - @pnpm/package-requester@16.0.2
  - @pnpm/types@7.10.0
  - @pnpm/core-loggers@6.1.4
  - @pnpm/fetcher-base@11.1.6
  - @pnpm/resolver-base@8.1.6
  - @pnpm/store-controller-types@11.0.12
  - @pnpm/cafs@3.0.14

## 12.1.10

### Patch Changes

- fa4f9133b: This fixes an issue introduced in pnpm v6.30.0.

  When a package is not linked to `node_modules`, no info message should be printed about it being "relinked" from the store [#4314](https://github.com/pnpm/pnpm/issues/4314).

  - @pnpm/package-requester@16.0.1

## 12.1.9

### Patch Changes

- 50e347d23: When checking whether a package is linked from the store, don't fail if the package has no `package.json` file.
  - @pnpm/package-requester@16.0.1

## 12.1.8

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/core-loggers@6.1.3
  - @pnpm/fetcher-base@11.1.5
  - @pnpm/package-requester@16.0.1
  - @pnpm/resolver-base@8.1.5
  - @pnpm/store-controller-types@11.0.11
  - @pnpm/cafs@3.0.13

## 12.1.7

### Patch Changes

- Updated dependencies [8ddcd5116]
  - @pnpm/package-requester@16.0.0

## 12.1.6

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/core-loggers@6.1.2
  - @pnpm/fetcher-base@11.1.4
  - @pnpm/package-requester@15.2.6
  - @pnpm/resolver-base@8.1.4
  - @pnpm/store-controller-types@11.0.10
  - @pnpm/cafs@3.0.12

## 12.1.5

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/core-loggers@6.1.1
  - @pnpm/fetcher-base@11.1.3
  - @pnpm/package-requester@15.2.5
  - @pnpm/resolver-base@8.1.3
  - @pnpm/store-controller-types@11.0.9
  - @pnpm/cafs@3.0.11

## 12.1.4

### Patch Changes

- d00e1fc6a: `pnpm store prune` should not fail if there are unexpected subdirectories in the content-addressable store.
- Updated dependencies [ba9b2eba1]
- Updated dependencies [77ff0898b]
- Updated dependencies [ba9b2eba1]
  - @pnpm/core-loggers@6.1.0
  - @pnpm/package-requester@15.2.4
  - @pnpm/types@7.7.0
  - @pnpm/fetcher-base@11.1.2
  - @pnpm/resolver-base@8.1.2
  - @pnpm/store-controller-types@11.0.8
  - @pnpm/cafs@3.0.10

## 12.1.3

### Patch Changes

- Updated dependencies [dbd8acfe9]
- Updated dependencies [119b3a908]
  - @pnpm/package-requester@15.2.3

## 12.1.2

### Patch Changes

- @pnpm/package-requester@15.2.2

## 12.1.1

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/core-loggers@6.0.6
  - @pnpm/fetcher-base@11.1.1
  - @pnpm/package-requester@15.2.1
  - @pnpm/resolver-base@8.1.1
  - @pnpm/store-controller-types@11.0.7
  - @pnpm/cafs@3.0.9

## 12.1.0

### Minor Changes

- 4ab87844a: Added support for "injected" dependencies.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/fetcher-base@11.1.0
  - @pnpm/resolver-base@8.1.0
  - @pnpm/package-requester@15.2.0
  - @pnpm/core-loggers@6.0.5
  - @pnpm/store-controller-types@11.0.6
  - @pnpm/cafs@3.0.8

## 12.0.15

### Patch Changes

- Updated dependencies [11a934da1]
  - @pnpm/package-requester@15.1.2

## 12.0.14

### Patch Changes

- Updated dependencies [31e01d9a9]
  - @pnpm/package-requester@15.1.1

## 12.0.13

### Patch Changes

- Updated dependencies [07e7b1c0c]
  - @pnpm/package-requester@15.1.0

## 12.0.12

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/core-loggers@6.0.4
  - @pnpm/fetcher-base@11.0.3
  - @pnpm/package-requester@15.0.7
  - @pnpm/resolver-base@8.0.4
  - @pnpm/store-controller-types@11.0.5
  - @pnpm/cafs@3.0.7

## 12.0.11

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/core-loggers@6.0.3
  - @pnpm/fetcher-base@11.0.2
  - @pnpm/package-requester@15.0.6
  - @pnpm/resolver-base@8.0.3
  - @pnpm/store-controller-types@11.0.4
  - @pnpm/cafs@3.0.6

## 12.0.10

### Patch Changes

- @pnpm/package-requester@15.0.5

## 12.0.9

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/package-requester@15.0.4
  - @pnpm/core-loggers@6.0.2
  - @pnpm/fetcher-base@11.0.1
  - @pnpm/resolver-base@8.0.2
  - @pnpm/store-controller-types@11.0.3
  - @pnpm/cafs@3.0.5

## 12.0.8

### Patch Changes

- Updated dependencies [a1a03d145]
  - @pnpm/package-requester@15.0.3

## 12.0.7

### Patch Changes

- @pnpm/package-requester@15.0.2

## 12.0.6

### Patch Changes

- Updated dependencies [ef0ca24be]
  - @pnpm/cafs@3.0.4
  - @pnpm/package-requester@15.0.1

## 12.0.5

### Patch Changes

- 3b147ced9: The temporary directory should be removed during prunning the store.
  - @pnpm/package-requester@15.0.0

## 12.0.4

### Patch Changes

- Updated dependencies [e6a2654a2]
- Updated dependencies [e6a2654a2]
- Updated dependencies [e6a2654a2]
  - @pnpm/fetcher-base@11.0.0
  - @pnpm/package-requester@15.0.0
  - @pnpm/cafs@3.0.3
  - @pnpm/store-controller-types@11.0.2

## 12.0.3

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/package-requester@14.0.3
  - @pnpm/core-loggers@6.0.1
  - @pnpm/fetcher-base@10.0.1
  - @pnpm/resolver-base@8.0.1
  - @pnpm/store-controller-types@11.0.1
  - @pnpm/cafs@3.0.2

## 12.0.2

### Patch Changes

- 6f198457d: Update rename-overwrite.
- e3d9b3215: Update make-empty-dir.
- Updated dependencies [6f198457d]
  - @pnpm/cafs@3.0.1
  - @pnpm/package-requester@14.0.2

## 12.0.1

### Patch Changes

- @pnpm/package-requester@14.0.1

## 12.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- 83645c8ed: Update ssri.
- Updated dependencies [97b986fbc]
- Updated dependencies [90487a3a8]
- Updated dependencies [83645c8ed]
  - @pnpm/cafs@3.0.0
  - @pnpm/core-loggers@6.0.0
  - @pnpm/fetcher-base@10.0.0
  - @pnpm/package-requester@14.0.0
  - @pnpm/resolver-base@8.0.0
  - @pnpm/store-controller-types@11.0.0
  - @pnpm/types@7.0.0

## 11.0.3

### Patch Changes

- @pnpm/package-requester@13.0.1

## 11.0.2

### Patch Changes

- @pnpm/package-requester@13.0.0

## 11.0.1

### Patch Changes

- 632352f26: Rename files with invalid names if linking fails.

## 11.0.0

### Major Changes

- 8d1dfa89c: Breaking changes to the store controller API.

  The options to `requestPackage()` and `fetchPackage()` changed.

### Patch Changes

- Updated dependencies [8d1dfa89c]
- Updated dependencies [8d1dfa89c]
  - @pnpm/package-requester@13.0.0
  - @pnpm/store-controller-types@10.0.0
  - @pnpm/cafs@2.1.0

## 10.1.18

### Patch Changes

- @pnpm/package-requester@12.2.2

## 10.1.17

### Patch Changes

- @pnpm/package-requester@12.2.2

## 10.1.16

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/core-loggers@5.0.3
  - @pnpm/fetcher-base@9.0.4
  - @pnpm/package-requester@12.2.2
  - @pnpm/resolver-base@7.1.1
  - @pnpm/store-controller-types@9.2.1
  - @pnpm/cafs@2.0.5

## 10.1.15

### Patch Changes

- @pnpm/package-requester@12.2.1

## 10.1.14

### Patch Changes

- @pnpm/package-requester@12.2.0

## 10.1.13

### Patch Changes

- @pnpm/package-requester@12.2.0

## 10.1.12

### Patch Changes

- @pnpm/package-requester@12.2.0

## 10.1.11

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/package-requester@12.2.0
  - @pnpm/resolver-base@7.1.0
  - @pnpm/store-controller-types@9.2.0
  - @pnpm/fetcher-base@9.0.3
  - @pnpm/cafs@2.0.4

## 10.1.10

### Patch Changes

- @pnpm/package-requester@12.1.4

## 10.1.9

### Patch Changes

- @pnpm/package-requester@12.1.4

## 10.1.8

### Patch Changes

- 09492b7b4: Update write-file-atomic to v3.
  - @pnpm/package-requester@12.1.3

## 10.1.7

### Patch Changes

- @pnpm/package-requester@12.1.3

## 10.1.6

### Patch Changes

- 01aecf038: Do not try to copy a file during linking, if the target already exists.
- Updated dependencies [b3059f4f8]
  - @pnpm/cafs@2.0.3
  - @pnpm/package-requester@12.1.3

## 10.1.5

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/core-loggers@5.0.2
  - @pnpm/fetcher-base@9.0.2
  - @pnpm/package-requester@12.1.2
  - @pnpm/resolver-base@7.0.5
  - @pnpm/store-controller-types@9.1.2
  - @pnpm/cafs@2.0.2

## 10.1.4

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/core-loggers@5.0.1
  - @pnpm/fetcher-base@9.0.1
  - @pnpm/package-requester@12.1.1
  - @pnpm/resolver-base@7.0.4
  - @pnpm/store-controller-types@9.1.1
  - @pnpm/cafs@2.0.1

## 10.1.3

### Patch Changes

- @pnpm/package-requester@12.1.0

## 10.1.2

### Patch Changes

- @pnpm/package-requester@12.1.0

## 10.1.1

### Patch Changes

- @pnpm/package-requester@12.1.0

## 10.1.0

### Minor Changes

- 0a6544043: A new field added to the package files index: `checkedAt`. `checkedAt` is the timestamp (number of milliseconds), when the file's content was verified the last time.

### Patch Changes

- Updated dependencies [0a6544043]
- Updated dependencies [0a6544043]
- Updated dependencies [0a6544043]
  - @pnpm/package-requester@12.1.0
  - @pnpm/store-controller-types@9.1.0
  - @pnpm/cafs@2.0.0
  - @pnpm/fetcher-base@9.0.0

## 10.0.2

### Patch Changes

- d94b19b39: Unless an EXDEV error is thrown during hard linking, always choose hard linking for importing packages from the store.

## 10.0.1

### Patch Changes

- 7f74cd173: Fixing a regression. Package should be imported when import method is being identified.

## 10.0.0

### Major Changes

- 86cd72de3: The `importPackage` function of the store controller returns the `importMethod` that was used to link the package to the virtual store. If importing was not needed, `importMethod` is `undefined`.

### Patch Changes

- Updated dependencies [86cd72de3]
- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0
  - @pnpm/store-controller-types@9.0.0
  - @pnpm/package-requester@12.0.13
  - @pnpm/cafs@1.0.8

## 9.1.8

### Patch Changes

- 6457562c4: When `package-import-method` is set to `auto`, cloning is only tried once. If it fails, it is not retried for other packages.
- 6457562c4: Report package importing once it actually succeeds.
  - @pnpm/package-requester@12.0.12

## 9.1.7

### Patch Changes

- Updated dependencies [501efdabd]
  - @pnpm/package-requester@12.0.12

## 9.1.6

### Patch Changes

- @pnpm/package-requester@12.0.11

## 9.1.5

### Patch Changes

- @pnpm/package-requester@12.0.10

## 9.1.4

### Patch Changes

- Updated dependencies [1525fff4c]
  - @pnpm/cafs@1.0.7
  - @pnpm/package-requester@12.0.9

## 9.1.3

### Patch Changes

- @pnpm/package-requester@12.0.8

## 9.1.2

### Patch Changes

- @pnpm/package-requester@12.0.8

## 9.1.1

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - @pnpm/cafs@1.0.6
  - @pnpm/package-requester@12.0.8

## 9.1.0

### Minor Changes

- 9a908bc07: Add packageImportMethod logger.

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0
  - @pnpm/package-requester@12.0.7

## 9.0.14

### Patch Changes

- @pnpm/package-requester@12.0.6

## 9.0.13

### Patch Changes

- @pnpm/package-requester@12.0.6

## 9.0.12

### Patch Changes

- @pnpm/package-requester@12.0.6

## 9.0.11

### Patch Changes

- @pnpm/package-requester@12.0.6

## 9.0.10

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - @pnpm/fetcher-base@8.0.2
  - @pnpm/package-requester@12.0.6
  - @pnpm/resolver-base@7.0.3
  - @pnpm/store-controller-types@8.0.2
  - @pnpm/cafs@1.0.5

## 9.0.9

### Patch Changes

- 1adacd41e: only scan directories when doing store prune

## 9.0.8

### Patch Changes

- @pnpm/package-requester@12.0.5

## 9.0.7

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/core-loggers@4.1.1
  - @pnpm/fetcher-base@8.0.1
  - @pnpm/package-requester@12.0.5
  - @pnpm/resolver-base@7.0.2
  - @pnpm/store-controller-types@8.0.1
  - @pnpm/cafs@1.0.4

## 9.0.6

### Patch Changes

- Updated dependencies [492805ee3]
  - @pnpm/cafs@1.0.3
  - @pnpm/package-requester@12.0.4

## 9.0.5

### Patch Changes

- @pnpm/package-requester@12.0.3

## 9.0.4

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [d3ddd023c]
- Updated dependencies [2ebb7af33]
  - @pnpm/cafs@1.0.2
  - @pnpm/package-requester@12.0.3
  - @pnpm/core-loggers@4.1.0

## 9.0.3

### Patch Changes

- Updated dependencies [a203bc138]
  - @pnpm/package-requester@12.0.2

## 9.0.2

### Patch Changes

- @pnpm/package-requester@12.0.1

## 9.0.1

### Patch Changes

- 429c5a560: If creating a hard-link to a file from the store fails, fall back to copying the file.
- Updated dependencies [bcd4aa1aa]
  - @pnpm/fetcher-base@8.0.0
  - @pnpm/package-requester@12.0.1
  - @pnpm/cafs@1.0.1

## 9.0.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- b6a82072e: Using a content-addressable filesystem for storing packages.
- 802d145fc: `getPackageLocation()` removed from store. Remove `inStoreLocation` from the result of `fetchPackage()`.
- a5febb913: The importPackage function of the store controller is importing packages directly from the side-effects cache.
- a5febb913: The upload function of the store controller accepts `opts.filesIndexFile` instead of `opts.packageId`.

### Minor Changes

- cbc2192f1: Don't try to create the dependency directory twice.
- f516d266c: Executables are saved into a separate directory inside the content-addressable storage.
- ecf2c6b7d: Prune unreferenced files from the store.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- c207d994f: Update rename-overwrite to v3.
- Updated dependencies [b5f66c0f2]
- Updated dependencies [9596774f2]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [f516d266c]
- Updated dependencies [7852deea3]
- Updated dependencies [da091c711]
- Updated dependencies [a7d20d927]
- Updated dependencies [42e6490d1]
- Updated dependencies [64bae33c4]
- Updated dependencies [a5febb913]
- Updated dependencies [b6a82072e]
- Updated dependencies [f93583d52]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [a5febb913]
- Updated dependencies [c207d994f]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [471149e66]
- Updated dependencies [42e6490d1]
  - @pnpm/package-requester@12.0.0
  - @pnpm/cafs@1.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/fetcher-base@7.0.0
  - @pnpm/types@6.0.0
  - @pnpm/core-loggers@4.0.2
  - @pnpm/resolver-base@7.0.1

## 9.0.0-alpha.5

### Major Changes

- a5febb913: The importPackage function of the store controller is importing packages directly from the side-effects cache.
- a5febb913: The upload function of the store controller accepts `opts.filesIndexFile` instead of `opts.packageId`.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [16d1ac0fd]
- Updated dependencies [a7d20d927]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/package-requester@12.0.0-alpha.5
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/cafs@1.0.0-alpha.5

## 9.0.0-alpha.4

### Major Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.

### Minor Changes

- ecf2c6b7: Prune unreferenced files from the store.

### Patch Changes

- Updated dependencies [da091c71]
- Updated dependencies [471149e6]
  - @pnpm/package-requester@12.0.0-alpha.4
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/cafs@1.0.0-alpha.4
  - @pnpm/core-loggers@4.0.2-alpha.0
  - @pnpm/fetcher-base@6.0.1-alpha.3
  - @pnpm/resolver-base@7.0.1-alpha.0

## 9.0.0-alpha.3

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [9596774f2]
- Updated dependencies [7852deea3]
  - @pnpm/package-requester@12.0.0-alpha.3
  - @pnpm/cafs@1.0.0-alpha.3

## 9.0.0-alpha.2

### Patch Changes

- c207d994f: Update rename-overwrite to v3.
- Updated dependencies [42e6490d1]
- Updated dependencies [64bae33c4]
- Updated dependencies [c207d994f]
- Updated dependencies [42e6490d1]
  - @pnpm/package-requester@12.0.0-alpha.2
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/cafs@1.0.0-alpha.2
  - @pnpm/fetcher-base@7.0.0-alpha.2

## 9.0.0-alpha.1

### Minor Changes

- 4f62d0383: Executables are saved into a separate directory inside the content-addressable storage.

### Patch Changes

- Updated dependencies [4f62d0383]
- Updated dependencies [f93583d52]
  - @pnpm/cafs@1.0.0-alpha.1
  - @pnpm/fetcher-base@7.0.0-alpha.1
  - @pnpm/package-requester@12.0.0-alpha.1
  - @pnpm/store-controller-types@8.0.0-alpha.1

## 9.0.0-alpha.0

### Major Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
- Updated dependencies [91c4b5954]
  - @pnpm/cafs@1.0.0-alpha.0
  - @pnpm/fetcher-base@7.0.0-alpha.0
  - @pnpm/package-requester@12.0.0-alpha.0
  - @pnpm/store-controller-types@8.0.0-alpha.0

## 8.1.0

### Minor Changes

- 907c63a48: The number of filesystem operations has been reduced.

### Patch Changes

- 907c63a48: Dependencies updated.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/package-requester@11.0.6
