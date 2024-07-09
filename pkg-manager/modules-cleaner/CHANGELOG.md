# @pnpm/modules-cleaner

## 15.1.7

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0
  - @pnpm/lockfile-utils@11.0.3
  - @pnpm/store-controller-types@18.1.2
  - @pnpm/filter-lockfile@9.0.8
  - @pnpm/lockfile-types@7.1.2
  - @pnpm/core-loggers@10.0.3
  - @pnpm/dependency-path@5.1.2
  - @pnpm/remove-bins@6.0.4

## 15.1.6

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/filter-lockfile@9.0.7
  - @pnpm/lockfile-types@7.1.1
  - @pnpm/lockfile-utils@11.0.2
  - @pnpm/core-loggers@10.0.2
  - @pnpm/dependency-path@5.1.1
  - @pnpm/remove-bins@6.0.3
  - @pnpm/store-controller-types@18.1.1

## 15.1.5

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/dependency-path@5.1.0
  - @pnpm/lockfile-types@7.1.0
  - @pnpm/filter-lockfile@9.0.6
  - @pnpm/lockfile-utils@11.0.1

## 15.1.4

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/store-controller-types@18.1.0

## 15.1.3

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-types@7.0.0
  - @pnpm/lockfile-utils@11.0.0
  - @pnpm/dependency-path@5.0.0
  - @pnpm/filter-lockfile@9.0.5
  - @pnpm/core-loggers@10.0.1
  - @pnpm/remove-bins@6.0.2
  - @pnpm/store-controller-types@18.0.1

## 15.1.2

### Patch Changes

- @pnpm/filter-lockfile@9.0.4
- @pnpm/remove-bins@6.0.1

## 15.1.1

### Patch Changes

- Updated dependencies [7a0536e]
  - @pnpm/lockfile-utils@10.1.1
  - @pnpm/filter-lockfile@9.0.3

## 15.1.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/dependency-path@4.0.0
  - @pnpm/lockfile-utils@10.1.0
  - @pnpm/filter-lockfile@9.0.2

## 15.0.1

### Patch Changes

- Updated dependencies [b7d2ed4]
  - @pnpm/filter-lockfile@9.0.1

## 15.0.0

### Major Changes

- cdd8365: Package ID does not contain the registry domain.
- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [cdd8365]
- Updated dependencies [89b396b]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [27a96a8]
- Updated dependencies [730929e]
- Updated dependencies [98a1266]
  - @pnpm/types@10.0.0
  - @pnpm/dependency-path@3.0.0
  - @pnpm/lockfile-utils@10.0.0
  - @pnpm/store-controller-types@18.0.0
  - @pnpm/filter-lockfile@9.0.0
  - @pnpm/lockfile-types@6.0.0
  - @pnpm/remove-bins@6.0.0
  - @pnpm/core-loggers@10.0.0
  - @pnpm/read-modules-dir@7.0.0

## 14.0.24

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/store-controller-types@17.2.0
  - @pnpm/lockfile-utils@9.0.5
  - @pnpm/filter-lockfile@8.1.6

## 14.0.23

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/lockfile-types@5.1.5
  - @pnpm/types@9.4.2
  - @pnpm/filter-lockfile@8.1.5
  - @pnpm/lockfile-utils@9.0.4
  - @pnpm/core-loggers@9.0.6
  - @pnpm/dependency-path@2.1.7
  - @pnpm/remove-bins@5.0.7
  - @pnpm/store-controller-types@17.1.4

## 14.0.22

### Patch Changes

- Updated dependencies
  - @pnpm/lockfile-types@5.1.4
  - @pnpm/types@9.4.1
  - @pnpm/filter-lockfile@8.1.4
  - @pnpm/lockfile-utils@9.0.3
  - @pnpm/core-loggers@9.0.5
  - @pnpm/dependency-path@2.1.6
  - @pnpm/remove-bins@5.0.6
  - @pnpm/store-controller-types@17.1.3

## 14.0.21

### Patch Changes

- f3cd0a61d: Installation should not fail if an empty `node_modules` directory cannot be removed [#7405](https://github.com/pnpm/pnpm/issues/7405).
- Updated dependencies [d5a176af7]
  - @pnpm/lockfile-utils@9.0.2
  - @pnpm/filter-lockfile@8.1.3

## 14.0.20

### Patch Changes

- 6558d1865: When `dedupe-direct-deps` is set to `true`, commands of dependencies should be deduplicated [#7359](https://github.com/pnpm/pnpm/pull/7359).

## 14.0.19

### Patch Changes

- Updated dependencies [b4194fe52]
  - @pnpm/lockfile-utils@9.0.1
  - @pnpm/filter-lockfile@8.1.2

## 14.0.18

### Patch Changes

- Updated dependencies [291607c5a]
  - @pnpm/store-controller-types@17.1.2

## 14.0.17

### Patch Changes

- Updated dependencies [4c2450208]
- Updated dependencies [7ea45afbe]
  - @pnpm/lockfile-utils@9.0.0
  - @pnpm/store-controller-types@17.1.1
  - @pnpm/filter-lockfile@8.1.1

## 14.0.16

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/store-controller-types@17.1.0
  - @pnpm/filter-lockfile@8.1.0
  - @pnpm/types@9.4.0
  - @pnpm/lockfile-types@5.1.3
  - @pnpm/lockfile-utils@8.0.7
  - @pnpm/core-loggers@9.0.4
  - @pnpm/dependency-path@2.1.5
  - @pnpm/remove-bins@5.0.5

## 14.0.15

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/filter-lockfile@8.0.10
  - @pnpm/lockfile-types@5.1.2
  - @pnpm/lockfile-utils@8.0.6
  - @pnpm/core-loggers@9.0.3
  - @pnpm/dependency-path@2.1.4
  - @pnpm/remove-bins@5.0.4
  - @pnpm/store-controller-types@17.0.1

## 14.0.14

### Patch Changes

- Updated dependencies [f394cfccd]
  - @pnpm/lockfile-utils@8.0.5
  - @pnpm/filter-lockfile@8.0.9

## 14.0.13

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/store-controller-types@17.0.0

## 14.0.12

### Patch Changes

- Updated dependencies [03cdccc6e]
  - @pnpm/store-controller-types@16.1.0

## 14.0.11

### Patch Changes

- @pnpm/store-controller-types@16.0.1

## 14.0.10

### Patch Changes

- Updated dependencies [494f87544]
- Updated dependencies [e9aa6f682]
  - @pnpm/store-controller-types@16.0.0
  - @pnpm/lockfile-utils@8.0.4
  - @pnpm/filter-lockfile@8.0.8

## 14.0.9

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/filter-lockfile@8.0.7
  - @pnpm/lockfile-types@5.1.1
  - @pnpm/lockfile-utils@8.0.3
  - @pnpm/core-loggers@9.0.2
  - @pnpm/dependency-path@2.1.3
  - @pnpm/remove-bins@5.0.3
  - @pnpm/store-controller-types@15.0.2

## 14.0.8

### Patch Changes

- 6fb5da19d: Replace ineffective use of ramda `difference` with better alternative

## 14.0.7

### Patch Changes

- Updated dependencies [d9da627cd]
  - @pnpm/lockfile-utils@8.0.2
  - @pnpm/filter-lockfile@8.0.6
  - @pnpm/remove-bins@5.0.2

## 14.0.6

### Patch Changes

- Updated dependencies [4b97f1f07]
  - @pnpm/read-modules-dir@6.0.1

## 14.0.5

### Patch Changes

- Updated dependencies [9c4ae87bd]
- Updated dependencies [a9e0b7cbf]
  - @pnpm/lockfile-types@5.1.0
  - @pnpm/types@9.1.0
  - @pnpm/filter-lockfile@8.0.5
  - @pnpm/lockfile-utils@8.0.1
  - @pnpm/core-loggers@9.0.1
  - @pnpm/dependency-path@2.1.2
  - @pnpm/remove-bins@5.0.1
  - @pnpm/store-controller-types@15.0.1

## 14.0.4

### Patch Changes

- Updated dependencies [d58cdb962]
  - @pnpm/lockfile-utils@8.0.0
  - @pnpm/filter-lockfile@8.0.4

## 14.0.3

### Patch Changes

- Updated dependencies [c0760128d]
  - @pnpm/dependency-path@2.1.1
  - @pnpm/filter-lockfile@8.0.3
  - @pnpm/lockfile-utils@7.0.1

## 14.0.2

### Patch Changes

- Updated dependencies [72ba638e3]
  - @pnpm/lockfile-utils@7.0.0
  - @pnpm/filter-lockfile@8.0.2

## 14.0.1

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
  - @pnpm/dependency-path@2.1.0
  - @pnpm/filter-lockfile@8.0.1
  - @pnpm/lockfile-utils@6.0.1

## 14.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [c92936158]
- Updated dependencies [ca8f51e60]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [0e26acb0f]
  - @pnpm/lockfile-types@5.0.0
  - @pnpm/lockfile-utils@6.0.0
  - @pnpm/dependency-path@2.0.0
  - @pnpm/store-controller-types@15.0.0
  - @pnpm/filter-lockfile@8.0.0
  - @pnpm/remove-bins@5.0.0
  - @pnpm/core-loggers@9.0.0
  - @pnpm/read-modules-dir@6.0.0
  - @pnpm/types@9.0.0

## 13.0.12

### Patch Changes

- @pnpm/lockfile-utils@5.0.7
- @pnpm/store-controller-types@14.3.1
- @pnpm/filter-lockfile@7.0.10

## 13.0.11

### Patch Changes

- Updated dependencies [d89d7a078]
  - @pnpm/dependency-path@1.1.3
  - @pnpm/filter-lockfile@7.0.9
  - @pnpm/lockfile-utils@5.0.6

## 13.0.10

### Patch Changes

- Updated dependencies [9247f6781]
  - @pnpm/dependency-path@1.1.2
  - @pnpm/filter-lockfile@7.0.8
  - @pnpm/lockfile-utils@5.0.5

## 13.0.9

### Patch Changes

- 1072ec128: Packages hoisted to the virtual store are not removed on repeat install, when the non-headless algorithm runs the installation.

## 13.0.8

### Patch Changes

- Updated dependencies [0f6e95872]
  - @pnpm/dependency-path@1.1.1
  - @pnpm/filter-lockfile@7.0.7
  - @pnpm/lockfile-utils@5.0.4

## 13.0.7

### Patch Changes

- Updated dependencies [891a8d763]
- Updated dependencies [c7b05cd9a]
- Updated dependencies [3ebce5db7]
  - @pnpm/store-controller-types@14.3.0
  - @pnpm/dependency-path@1.1.0
  - @pnpm/filter-lockfile@7.0.6
  - @pnpm/lockfile-utils@5.0.3
  - @pnpm/remove-bins@4.0.5

## 13.0.6

### Patch Changes

- Updated dependencies [b77651d14]
- Updated dependencies [2458741fa]
  - @pnpm/types@8.10.0
  - @pnpm/store-controller-types@14.2.0
  - @pnpm/filter-lockfile@7.0.5
  - @pnpm/lockfile-types@4.3.6
  - @pnpm/lockfile-utils@5.0.2
  - @pnpm/core-loggers@8.0.3
  - @pnpm/dependency-path@1.0.1
  - @pnpm/remove-bins@4.0.4

## 13.0.5

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/dependency-path@1.0.0
  - @pnpm/filter-lockfile@7.0.4
  - @pnpm/lockfile-utils@5.0.1

## 13.0.4

### Patch Changes

- @pnpm/remove-bins@4.0.3

## 13.0.3

### Patch Changes

- Updated dependencies [ecc8794bb]
- Updated dependencies [ecc8794bb]
  - @pnpm/lockfile-utils@5.0.0
  - @pnpm/filter-lockfile@7.0.3

## 13.0.2

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - @pnpm/core-loggers@8.0.2
  - dependency-path@9.2.8
  - @pnpm/filter-lockfile@7.0.2
  - @pnpm/lockfile-types@4.3.5
  - @pnpm/lockfile-utils@4.2.8
  - @pnpm/remove-bins@4.0.2
  - @pnpm/store-controller-types@14.1.5

## 13.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - @pnpm/core-loggers@8.0.1
  - dependency-path@9.2.7
  - @pnpm/filter-lockfile@7.0.1
  - @pnpm/lockfile-types@4.3.4
  - @pnpm/lockfile-utils@4.2.7
  - @pnpm/remove-bins@4.0.1
  - @pnpm/store-controller-types@14.1.4

## 13.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [f884689e0]
- Updated dependencies [a236ecf57]
  - @pnpm/core-loggers@8.0.0
  - @pnpm/filter-lockfile@7.0.0
  - @pnpm/read-modules-dir@5.0.0
  - @pnpm/remove-bins@4.0.0

## 12.0.25

### Patch Changes

- Updated dependencies [3ae888c28]
  - @pnpm/core-loggers@7.1.0
  - @pnpm/remove-bins@3.0.13
  - @pnpm/filter-lockfile@6.0.22

## 12.0.24

### Patch Changes

- @pnpm/filter-lockfile@6.0.21
- @pnpm/remove-bins@3.0.12

## 12.0.23

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/core-loggers@7.0.8
  - dependency-path@9.2.6
  - @pnpm/filter-lockfile@6.0.20
  - @pnpm/lockfile-types@4.3.3
  - @pnpm/lockfile-utils@4.2.6
  - @pnpm/remove-bins@3.0.11
  - @pnpm/store-controller-types@14.1.3

## 12.0.22

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/core-loggers@7.0.7
  - dependency-path@9.2.5
  - @pnpm/filter-lockfile@6.0.19
  - @pnpm/lockfile-types@4.3.2
  - @pnpm/lockfile-utils@4.2.5
  - @pnpm/remove-bins@3.0.10
  - @pnpm/store-controller-types@14.1.2

## 12.0.21

### Patch Changes

- Updated dependencies [1beb1b4bd]
  - @pnpm/filter-lockfile@6.0.18

## 12.0.20

### Patch Changes

- @pnpm/remove-bins@3.0.9

## 12.0.19

### Patch Changes

- Updated dependencies [32915f0e4]
  - @pnpm/store-controller-types@14.1.1
  - @pnpm/lockfile-utils@4.2.4
  - @pnpm/filter-lockfile@6.0.17

## 12.0.18

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
  - @pnpm/filter-lockfile@6.0.16
  - @pnpm/lockfile-utils@4.2.3
  - @pnpm/store-controller-types@14.1.0

## 12.0.17

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/core-loggers@7.0.6
  - dependency-path@9.2.4
  - @pnpm/filter-lockfile@6.0.15
  - @pnpm/lockfile-types@4.3.1
  - @pnpm/lockfile-utils@4.2.2
  - @pnpm/remove-bins@3.0.8
  - @pnpm/store-controller-types@14.0.2

## 12.0.16

### Patch Changes

- Updated dependencies [c83f40c10]
  - @pnpm/lockfile-utils@4.2.1
  - @pnpm/filter-lockfile@6.0.14

## 12.0.15

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-types@4.3.0
  - @pnpm/lockfile-utils@4.2.0
  - @pnpm/filter-lockfile@6.0.13

## 12.0.14

### Patch Changes

- Updated dependencies [e3f4d131c]
  - @pnpm/lockfile-utils@4.1.0
  - @pnpm/filter-lockfile@6.0.12

## 12.0.13

### Patch Changes

- dependency-path@9.2.3
- @pnpm/filter-lockfile@6.0.11
- @pnpm/lockfile-utils@4.0.10

## 12.0.12

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
  - @pnpm/filter-lockfile@6.0.10
  - @pnpm/lockfile-utils@4.0.9
  - @pnpm/remove-bins@3.0.7

## 12.0.11

### Patch Changes

- Updated dependencies [fc581d371]
  - dependency-path@9.2.2
  - @pnpm/filter-lockfile@6.0.9
  - @pnpm/lockfile-utils@4.0.8

## 12.0.10

### Patch Changes

- Updated dependencies [d01c32355]
- Updated dependencies [8e5b77ef6]
- Updated dependencies [8e5b77ef6]
  - @pnpm/lockfile-types@4.2.0
  - @pnpm/types@8.4.0
  - @pnpm/filter-lockfile@6.0.8
  - @pnpm/lockfile-utils@4.0.7
  - @pnpm/core-loggers@7.0.5
  - dependency-path@9.2.1
  - @pnpm/remove-bins@3.0.6
  - @pnpm/store-controller-types@14.0.1

## 12.0.9

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [c635f9fc1]
- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/lockfile-types@4.1.0
  - dependency-path@9.2.0
  - @pnpm/store-controller-types@14.0.0
  - @pnpm/core-loggers@7.0.4
  - @pnpm/filter-lockfile@6.0.7
  - @pnpm/lockfile-utils@4.0.6
  - @pnpm/remove-bins@3.0.5

## 12.0.8

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [725636a90]
  - @pnpm/types@8.2.0
  - dependency-path@9.1.4
  - @pnpm/core-loggers@7.0.3
  - @pnpm/filter-lockfile@6.0.6
  - @pnpm/lockfile-types@4.0.3
  - @pnpm/lockfile-utils@4.0.5
  - @pnpm/remove-bins@3.0.4
  - @pnpm/store-controller-types@13.0.4

## 12.0.7

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/core-loggers@7.0.2
  - dependency-path@9.1.3
  - @pnpm/filter-lockfile@6.0.5
  - @pnpm/lockfile-types@4.0.2
  - @pnpm/lockfile-utils@4.0.4
  - @pnpm/remove-bins@3.0.3
  - @pnpm/store-controller-types@13.0.3

## 12.0.6

### Patch Changes

- Updated dependencies [6756c2b02]
  - @pnpm/store-controller-types@13.0.2

## 12.0.5

### Patch Changes

- Updated dependencies [c57695550]
  - dependency-path@9.1.2
  - @pnpm/filter-lockfile@6.0.4
  - @pnpm/lockfile-utils@4.0.3

## 12.0.4

### Patch Changes

- Updated dependencies [52b0576af]
  - @pnpm/filter-lockfile@6.0.3

## 12.0.3

### Patch Changes

- 0075fcd23: Do not remove hoisted dependencies, when pruneDirectDependencies is set to `true`.

## 12.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/core-loggers@7.0.1
  - dependency-path@9.1.1
  - @pnpm/filter-lockfile@6.0.2
  - @pnpm/lockfile-types@4.0.1
  - @pnpm/lockfile-utils@4.0.2
  - @pnpm/remove-bins@3.0.2
  - @pnpm/store-controller-types@13.0.1

## 12.0.1

### Patch Changes

- Updated dependencies [0a70aedb1]
- Updated dependencies [688b0eaff]
  - dependency-path@9.1.0
  - @pnpm/lockfile-utils@4.0.1
  - @pnpm/filter-lockfile@6.0.1
  - @pnpm/remove-bins@3.0.1

## 12.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [faf830b8f]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - dependency-path@9.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/filter-lockfile@6.0.0
  - @pnpm/lockfile-types@4.0.0
  - @pnpm/lockfile-utils@4.0.0
  - @pnpm/read-modules-dir@4.0.0
  - @pnpm/remove-bins@3.0.0
  - @pnpm/store-controller-types@13.0.0

## 11.0.23

### Patch Changes

- Updated dependencies [70ba51da9]
- Updated dependencies [5c525db13]
  - @pnpm/filter-lockfile@5.0.19
  - @pnpm/store-controller-types@12.0.0
  - @pnpm/remove-bins@2.0.14

## 11.0.22

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-types@3.2.0
  - @pnpm/types@7.10.0
  - @pnpm/filter-lockfile@5.0.18
  - @pnpm/lockfile-utils@3.2.1
  - @pnpm/core-loggers@6.1.4
  - dependency-path@8.0.11
  - @pnpm/remove-bins@2.0.13
  - @pnpm/store-controller-types@11.0.12

## 11.0.21

### Patch Changes

- Updated dependencies [cdc521cfa]
  - @pnpm/lockfile-utils@3.2.0
  - @pnpm/filter-lockfile@5.0.17

## 11.0.20

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/core-loggers@6.1.3
  - dependency-path@8.0.10
  - @pnpm/filter-lockfile@5.0.16
  - @pnpm/lockfile-types@3.1.5
  - @pnpm/lockfile-utils@3.1.6
  - @pnpm/remove-bins@2.0.12
  - @pnpm/store-controller-types@11.0.11

## 11.0.19

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/core-loggers@6.1.2
  - dependency-path@8.0.9
  - @pnpm/filter-lockfile@5.0.15
  - @pnpm/lockfile-types@3.1.4
  - @pnpm/lockfile-utils@3.1.5
  - @pnpm/remove-bins@2.0.11
  - @pnpm/store-controller-types@11.0.10

## 11.0.18

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/core-loggers@6.1.1
  - dependency-path@8.0.8
  - @pnpm/filter-lockfile@5.0.14
  - @pnpm/lockfile-types@3.1.3
  - @pnpm/lockfile-utils@3.1.4
  - @pnpm/remove-bins@2.0.10
  - @pnpm/store-controller-types@11.0.9

## 11.0.17

### Patch Changes

- Updated dependencies [ba9b2eba1]
- Updated dependencies [ba9b2eba1]
  - @pnpm/core-loggers@6.1.0
  - @pnpm/types@7.7.0
  - @pnpm/remove-bins@2.0.9
  - dependency-path@8.0.7
  - @pnpm/filter-lockfile@5.0.13
  - @pnpm/lockfile-types@3.1.2
  - @pnpm/lockfile-utils@3.1.3
  - @pnpm/store-controller-types@11.0.8

## 11.0.16

### Patch Changes

- Updated dependencies [3cf543fc1]
  - @pnpm/lockfile-utils@3.1.2
  - @pnpm/filter-lockfile@5.0.12

## 11.0.15

### Patch Changes

- @pnpm/filter-lockfile@5.0.11

## 11.0.14

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/core-loggers@6.0.6
  - dependency-path@8.0.6
  - @pnpm/filter-lockfile@5.0.10
  - @pnpm/lockfile-types@3.1.1
  - @pnpm/lockfile-utils@3.1.1
  - @pnpm/remove-bins@2.0.8
  - @pnpm/store-controller-types@11.0.7

## 11.0.13

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/lockfile-types@3.1.0
  - @pnpm/lockfile-utils@3.1.0
  - @pnpm/core-loggers@6.0.5
  - dependency-path@8.0.5
  - @pnpm/filter-lockfile@5.0.9
  - @pnpm/remove-bins@2.0.7
  - @pnpm/store-controller-types@11.0.6

## 11.0.12

### Patch Changes

- Updated dependencies [0d4a7c69e]
  - @pnpm/remove-bins@2.0.6

## 11.0.11

### Patch Changes

- @pnpm/remove-bins@2.0.5

## 11.0.10

### Patch Changes

- Updated dependencies [71aab049d]
  - @pnpm/read-modules-dir@3.0.1

## 11.0.9

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/core-loggers@6.0.4
  - dependency-path@8.0.4
  - @pnpm/filter-lockfile@5.0.8
  - @pnpm/lockfile-utils@3.0.8
  - @pnpm/remove-bins@2.0.4
  - @pnpm/store-controller-types@11.0.5

## 11.0.8

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/core-loggers@6.0.3
  - dependency-path@8.0.3
  - @pnpm/filter-lockfile@5.0.7
  - @pnpm/lockfile-utils@3.0.7
  - @pnpm/remove-bins@2.0.3
  - @pnpm/store-controller-types@11.0.4

## 11.0.7

### Patch Changes

- Updated dependencies [6c418943c]
  - dependency-path@8.0.2
  - @pnpm/filter-lockfile@5.0.6
  - @pnpm/lockfile-utils@3.0.6

## 11.0.6

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/core-loggers@6.0.2
  - dependency-path@8.0.1
  - @pnpm/filter-lockfile@5.0.5
  - @pnpm/lockfile-utils@3.0.5
  - @pnpm/remove-bins@2.0.2
  - @pnpm/store-controller-types@11.0.3

## 11.0.5

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/filter-lockfile@5.0.4
  - @pnpm/lockfile-utils@3.0.4

## 11.0.4

### Patch Changes

- Updated dependencies [20e2f235d]
  - dependency-path@8.0.0
  - @pnpm/filter-lockfile@5.0.3
  - @pnpm/lockfile-utils@3.0.3

## 11.0.3

### Patch Changes

- @pnpm/store-controller-types@11.0.2

## 11.0.2

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/core-loggers@6.0.1
  - dependency-path@7.0.1
  - @pnpm/filter-lockfile@5.0.2
  - @pnpm/lockfile-utils@3.0.2
  - @pnpm/remove-bins@2.0.1
  - @pnpm/store-controller-types@11.0.1

## 11.0.1

### Patch Changes

- Updated dependencies [9ceab68f0]
  - dependency-path@7.0.0
  - @pnpm/filter-lockfile@5.0.1
  - @pnpm/lockfile-utils@3.0.1

## 11.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Minor Changes

- 78470a32d: `prune()` accepts a new option: `pruneVirtualStore`. When `pruneVirtualStore` is `true`, any unreferenced packages are removed from the virtual store (from `node_modules/.pnpm`).

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [6871d74b2]
- Updated dependencies [90487a3a8]
- Updated dependencies [e4efddbd2]
- Updated dependencies [f2bb5cbeb]
  - @pnpm/core-loggers@6.0.0
  - dependency-path@6.0.0
  - @pnpm/filter-lockfile@5.0.0
  - @pnpm/lockfile-types@3.0.0
  - @pnpm/lockfile-utils@3.0.0
  - @pnpm/read-modules-dir@3.0.0
  - @pnpm/remove-bins@2.0.0
  - @pnpm/store-controller-types@11.0.0
  - @pnpm/types@7.0.0

## 10.0.23

### Patch Changes

- @pnpm/remove-bins@1.0.12

## 10.0.22

### Patch Changes

- Updated dependencies [8d1dfa89c]
  - @pnpm/store-controller-types@10.0.0

## 10.0.21

### Patch Changes

- @pnpm/remove-bins@1.0.11

## 10.0.20

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [9ad8c27bf]
  - @pnpm/lockfile-types@2.2.0
  - @pnpm/types@6.4.0
  - @pnpm/filter-lockfile@4.0.17
  - @pnpm/lockfile-utils@2.0.22
  - @pnpm/core-loggers@5.0.3
  - dependency-path@5.1.1
  - @pnpm/remove-bins@1.0.10
  - @pnpm/store-controller-types@9.2.1

## 10.0.19

### Patch Changes

- Updated dependencies [af897c324]
  - @pnpm/filter-lockfile@4.0.16

## 10.0.18

### Patch Changes

- Updated dependencies [e27dcf0dc]
  - dependency-path@5.1.0
  - @pnpm/filter-lockfile@4.0.15
  - @pnpm/lockfile-utils@2.0.21

## 10.0.17

### Patch Changes

- 130970393: Symlinks to hoisted dependencies should be removed during pruning.

## 10.0.16

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/store-controller-types@9.2.0
  - @pnpm/lockfile-utils@2.0.20
  - @pnpm/filter-lockfile@4.0.14

## 10.0.15

### Patch Changes

- @pnpm/filter-lockfile@4.0.13
- @pnpm/remove-bins@1.0.9

## 10.0.14

### Patch Changes

- Updated dependencies [39142e2ad]
  - dependency-path@5.0.6
  - @pnpm/filter-lockfile@4.0.12
  - @pnpm/lockfile-utils@2.0.19

## 10.0.13

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/lockfile-types@2.1.1
  - @pnpm/types@6.3.1
  - @pnpm/filter-lockfile@4.0.11
  - @pnpm/lockfile-utils@2.0.18
  - @pnpm/core-loggers@5.0.2
  - dependency-path@5.0.5
  - @pnpm/remove-bins@1.0.8
  - @pnpm/store-controller-types@9.1.2

## 10.0.12

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [d54043ee4]
  - @pnpm/lockfile-types@2.1.0
  - @pnpm/types@6.3.0
  - @pnpm/filter-lockfile@4.0.10
  - @pnpm/lockfile-utils@2.0.17
  - @pnpm/core-loggers@5.0.1
  - dependency-path@5.0.4
  - @pnpm/remove-bins@1.0.7
  - @pnpm/store-controller-types@9.1.1

## 10.0.11

### Patch Changes

- Updated dependencies [0a6544043]
  - @pnpm/store-controller-types@9.1.0

## 10.0.10

### Patch Changes

- Updated dependencies [86cd72de3]
- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0
  - @pnpm/store-controller-types@9.0.0
  - @pnpm/remove-bins@1.0.6
  - @pnpm/filter-lockfile@4.0.9

## 10.0.9

### Patch Changes

- @pnpm/filter-lockfile@4.0.8
- @pnpm/remove-bins@1.0.5

## 10.0.8

### Patch Changes

- @pnpm/remove-bins@1.0.4

## 10.0.7

### Patch Changes

- @pnpm/filter-lockfile@4.0.7

## 10.0.6

### Patch Changes

- Updated dependencies [24af41f20]
  - @pnpm/read-modules-dir@2.0.3

## 10.0.5

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [1140ef721]
- Updated dependencies [a2ef8084f]
  - @pnpm/lockfile-utils@2.0.16
  - dependency-path@5.0.3
  - @pnpm/filter-lockfile@4.0.6
  - @pnpm/read-modules-dir@2.0.2
  - @pnpm/remove-bins@1.0.3

## 10.0.4

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0
  - @pnpm/remove-bins@1.0.2
  - @pnpm/filter-lockfile@4.0.5

## 10.0.3

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - dependency-path@5.0.2
  - @pnpm/filter-lockfile@4.0.4
  - @pnpm/lockfile-utils@2.0.15
  - @pnpm/remove-bins@1.0.1
  - @pnpm/store-controller-types@8.0.2

## 10.0.2

### Patch Changes

- 57d08f303: Remove global bins when unlinking.
- Updated dependencies [57d08f303]
  - @pnpm/remove-bins@1.0.0

## 10.0.1

### Patch Changes

- Updated dependencies [1520e3d6f]
  - @pnpm/package-bins@4.0.6

## 10.0.0

### Major Changes

- 71a8c8ce3: Replaced `hoistedAliases` with `hoistedDependencies`.

  Added `publicHoistedModulesDir` option.

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/core-loggers@4.1.1
  - dependency-path@5.0.1
  - @pnpm/filter-lockfile@4.0.3
  - @pnpm/lockfile-utils@2.0.14
  - @pnpm/package-bins@4.0.5
  - @pnpm/read-package-json@3.1.2
  - @pnpm/store-controller-types@8.0.1

## 9.0.2

### Patch Changes

- Updated dependencies [41d92948b]
  - dependency-path@5.0.0
  - @pnpm/filter-lockfile@4.0.2
  - @pnpm/lockfile-utils@2.0.13

## 9.0.1

### Patch Changes

- Updated dependencies [2ebb7af33]
  - @pnpm/core-loggers@4.1.0
  - @pnpm/filter-lockfile@4.0.1

## 9.0.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- 9fbb74ecb: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Minor Changes

- 7179cc560: Don't try to remove empty branches of a directory tree, when pruning `node_modules`.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [c25cccdad]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [f516d266c]
- Updated dependencies [da091c711]
- Updated dependencies [42e6490d1]
- Updated dependencies [2485eaf60]
- Updated dependencies [a5febb913]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [6a8a97eee]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/filter-lockfile@4.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lockfile-types@2.0.1
  - @pnpm/core-loggers@4.0.2
  - dependency-path@4.0.7
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/package-bins@4.0.4
  - @pnpm/read-modules-dir@2.0.2
  - @pnpm/read-package-json@3.1.1

## 9.0.0-alpha.5

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [c25cccdad]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [2485eaf60]
- Updated dependencies [a5febb913]
- Updated dependencies [6a8a97eee]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/filter-lockfile@4.0.0-alpha.2
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/lockfile-types@2.0.1-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.1

## 9.0.0-alpha.4

### Major Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Minor Changes

- 7179cc56: Don't try to remove empty branches of a directory tree, when pruning `node_modules`.

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/core-loggers@4.0.2-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/filter-lockfile@3.2.3-alpha.1
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/package-bins@4.0.4-alpha.0
  - @pnpm/read-package-json@3.1.1-alpha.0

## 9.0.0-alpha.3

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- @pnpm/filter-lockfile@3.2.3-alpha.0

## 8.0.17-alpha.2

### Patch Changes

- Updated dependencies [42e6490d1]
  - @pnpm/store-controller-types@8.0.0-alpha.2

## 8.0.17-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/store-controller-types@8.0.0-alpha.1

## 8.0.17-alpha.0

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/store-controller-types@8.0.0-alpha.0

## 8.0.16

### Patch Changes

- Updated dependencies [907c63a48]
  - @pnpm/filter-lockfile@3.2.2
  - @pnpm/lockfile-utils@2.0.11
