# @pnpm/read-projects-context

## 9.1.6

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0
  - @pnpm/normalize-registries@6.0.3
  - @pnpm/lockfile-file@9.1.2
  - @pnpm/modules-yaml@13.1.3

## 9.1.5

### Patch Changes

- 13e55b2: If install is performed on a subset of workspace projects, always create an up-to-date lockfile first. So, a partial install can be performed only on a fully resolved (non-partial) lockfile [#8165](https://github.com/pnpm/pnpm/issues/8165).
- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/normalize-registries@6.0.2
  - @pnpm/lockfile-file@9.1.1
  - @pnpm/modules-yaml@13.1.2

## 9.1.4

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/lockfile-file@9.1.0

## 9.1.3

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/normalize-registries@6.0.1
  - @pnpm/lockfile-file@9.0.6
  - @pnpm/modules-yaml@13.1.1

## 9.1.2

### Patch Changes

- @pnpm/lockfile-file@9.0.5

## 9.1.1

### Patch Changes

- @pnpm/lockfile-file@9.0.4

## 9.1.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/modules-yaml@13.1.0
  - @pnpm/lockfile-file@9.0.3

## 9.0.2

### Patch Changes

- Updated dependencies [c969f37]
  - @pnpm/lockfile-file@9.0.2

## 9.0.1

### Patch Changes

- Updated dependencies [2cbf7b7]
- Updated dependencies [6b6ca69]
  - @pnpm/lockfile-file@9.0.1

## 9.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [f67ad31]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/normalize-registries@6.0.0
  - @pnpm/modules-yaml@13.0.0
  - @pnpm/lockfile-file@9.0.0

## 8.0.11

### Patch Changes

- Updated dependencies [d349bc3a2]
  - @pnpm/modules-yaml@12.1.7

## 8.0.10

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2
  - @pnpm/lockfile-file@8.1.6
  - @pnpm/normalize-registries@5.0.6
  - @pnpm/modules-yaml@12.1.6

## 8.0.9

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/lockfile-file@8.1.5
  - @pnpm/normalize-registries@5.0.5
  - @pnpm/modules-yaml@12.1.5

## 8.0.8

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/normalize-registries@5.0.4
  - @pnpm/lockfile-file@8.1.4
  - @pnpm/modules-yaml@12.1.4

## 8.0.7

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/normalize-registries@5.0.3
  - @pnpm/lockfile-file@8.1.3
  - @pnpm/modules-yaml@12.1.3

## 8.0.6

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/normalize-registries@5.0.2
  - @pnpm/lockfile-file@8.1.2
  - @pnpm/modules-yaml@12.1.2

## 8.0.5

### Patch Changes

- @pnpm/lockfile-file@8.1.1

## 8.0.4

### Patch Changes

- Updated dependencies [9c4ae87bd]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [9c4ae87bd]
  - @pnpm/lockfile-file@8.1.0
  - @pnpm/types@9.1.0
  - @pnpm/normalize-registries@5.0.1
  - @pnpm/modules-yaml@12.1.1

## 8.0.3

### Patch Changes

- Updated dependencies [e6b83c84e]
  - @pnpm/modules-yaml@12.1.0

## 8.0.2

### Patch Changes

- Updated dependencies [c0760128d]
  - @pnpm/lockfile-file@8.0.2

## 8.0.1

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
  - @pnpm/lockfile-file@8.0.1

## 8.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [158d8cf22]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [417c8ac59]
  - @pnpm/lockfile-file@8.0.0
  - @pnpm/normalize-registries@5.0.0
  - @pnpm/modules-yaml@12.0.0
  - @pnpm/types@9.0.0

## 7.0.12

### Patch Changes

- Updated dependencies [787c43dcc]
  - @pnpm/lockfile-file@7.0.6

## 7.0.11

### Patch Changes

- Updated dependencies [ed946c73e]
  - @pnpm/lockfile-file@7.0.5

## 7.0.10

### Patch Changes

- @pnpm/lockfile-file@7.0.4

## 7.0.9

### Patch Changes

- @pnpm/lockfile-file@7.0.3

## 7.0.8

### Patch Changes

- Updated dependencies [9a68ebbae]
  - @pnpm/lockfile-file@7.0.2

## 7.0.7

### Patch Changes

- @pnpm/lockfile-file@7.0.1

## 7.0.6

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/lockfile-file@7.0.0

## 7.0.5

### Patch Changes

- Updated dependencies [b77651d14]
- Updated dependencies [2458741fa]
  - @pnpm/types@8.10.0
  - @pnpm/modules-yaml@11.1.0
  - @pnpm/normalize-registries@4.0.3
  - @pnpm/lockfile-file@6.0.5

## 7.0.4

### Patch Changes

- @pnpm/lockfile-file@6.0.4

## 7.0.3

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/lockfile-file@6.0.3

## 7.0.2

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - @pnpm/lockfile-file@6.0.2
  - @pnpm/modules-yaml@11.0.2
  - @pnpm/normalize-registries@4.0.2

## 7.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - @pnpm/lockfile-file@6.0.1
  - @pnpm/modules-yaml@11.0.1
  - @pnpm/normalize-registries@4.0.1

## 7.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [72f7d6b3b]
- Updated dependencies [f884689e0]
  - @pnpm/modules-yaml@11.0.0
  - @pnpm/lockfile-file@6.0.0
  - @pnpm/normalize-registries@4.0.0

## 6.0.19

### Patch Changes

- Updated dependencies [7c296fe9b]
  - @pnpm/lockfile-file@5.3.8

## 6.0.18

### Patch Changes

- @pnpm/lockfile-file@5.3.7

## 6.0.17

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/lockfile-file@5.3.6
  - @pnpm/modules-yaml@10.0.8
  - @pnpm/normalize-registries@3.0.8

## 6.0.16

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/lockfile-file@5.3.5
  - @pnpm/modules-yaml@10.0.7
  - @pnpm/normalize-registries@3.0.7

## 6.0.15

### Patch Changes

- Updated dependencies [0373af22e]
  - @pnpm/lockfile-file@5.3.4

## 6.0.14

### Patch Changes

- Updated dependencies [1e5482da4]
  - @pnpm/lockfile-file@5.3.3

## 6.0.13

### Patch Changes

- Updated dependencies [8103f92bd]
  - @pnpm/lockfile-file@5.3.2

## 6.0.12

### Patch Changes

- Updated dependencies [44544b493]
- Updated dependencies [c90798461]
  - @pnpm/lockfile-file@5.3.1
  - @pnpm/types@8.5.0
  - @pnpm/modules-yaml@10.0.6
  - @pnpm/normalize-registries@3.0.6

## 6.0.11

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-file@5.3.0

## 6.0.10

### Patch Changes

- Updated dependencies [4fa1091c8]
  - @pnpm/lockfile-file@5.2.0

## 6.0.9

### Patch Changes

- Updated dependencies [ab684d77e]
  - @pnpm/lockfile-file@5.1.4

## 6.0.8

### Patch Changes

- Updated dependencies [5f643f23b]
  - @pnpm/lockfile-file@5.1.3

## 6.0.7

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/lockfile-file@5.1.2
  - @pnpm/modules-yaml@10.0.5
  - @pnpm/normalize-registries@3.0.5

## 6.0.6

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/lockfile-file@5.1.1
  - @pnpm/modules-yaml@10.0.4
  - @pnpm/normalize-registries@3.0.4

## 6.0.5

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [56cf04cb3]
  - @pnpm/types@8.2.0
  - @pnpm/lockfile-file@5.1.0
  - @pnpm/modules-yaml@10.0.3
  - @pnpm/normalize-registries@3.0.3

## 6.0.4

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/lockfile-file@5.0.4
  - @pnpm/modules-yaml@10.0.2
  - @pnpm/normalize-registries@3.0.2

## 6.0.3

### Patch Changes

- Updated dependencies [52b0576af]
  - @pnpm/lockfile-file@5.0.3

## 6.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/lockfile-file@5.0.2
  - @pnpm/modules-yaml@10.0.1
  - @pnpm/normalize-registries@3.0.1

## 6.0.1

### Patch Changes

- @pnpm/lockfile-file@5.0.1

## 6.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/lockfile-file@5.0.0
  - @pnpm/modules-yaml@10.0.0
  - @pnpm/normalize-registries@3.0.0

## 5.0.19

### Patch Changes

- @pnpm/lockfile-file@4.3.1

## 5.0.18

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-file@4.3.0
  - @pnpm/types@7.10.0
  - @pnpm/modules-yaml@9.1.1
  - @pnpm/normalize-registries@2.0.13

## 5.0.17

### Patch Changes

- Updated dependencies [cdc521cfa]
  - @pnpm/modules-yaml@9.1.0

## 5.0.16

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/lockfile-file@4.2.6
  - @pnpm/modules-yaml@9.0.11
  - @pnpm/normalize-registries@2.0.12

## 5.0.15

### Patch Changes

- Updated dependencies [7375396db]
  - @pnpm/modules-yaml@9.0.10

## 5.0.14

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/lockfile-file@4.2.5
  - @pnpm/modules-yaml@9.0.9
  - @pnpm/normalize-registries@2.0.11

## 5.0.13

### Patch Changes

- Updated dependencies [eb9ebd0f3]
- Updated dependencies [eb9ebd0f3]
  - @pnpm/lockfile-file@4.2.4

## 5.0.12

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/lockfile-file@4.2.3
  - @pnpm/modules-yaml@9.0.8
  - @pnpm/normalize-registries@2.0.10

## 5.0.11

### Patch Changes

- Updated dependencies [30bfca967]
- Updated dependencies [ba9b2eba1]
  - @pnpm/normalize-registries@2.0.9
  - @pnpm/types@7.7.0
  - @pnpm/lockfile-file@4.2.2
  - @pnpm/modules-yaml@9.0.7

## 5.0.10

### Patch Changes

- Updated dependencies [46aaf7108]
  - @pnpm/normalize-registries@2.0.8

## 5.0.9

### Patch Changes

- Updated dependencies [a7ff2d5ce]
  - @pnpm/normalize-registries@2.0.7

## 5.0.8

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/lockfile-file@4.2.1
  - @pnpm/modules-yaml@9.0.6
  - @pnpm/normalize-registries@2.0.6

## 5.0.7

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/lockfile-file@4.2.0
  - @pnpm/modules-yaml@9.0.5
  - @pnpm/normalize-registries@2.0.5

## 5.0.6

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/lockfile-file@4.1.1
  - @pnpm/modules-yaml@9.0.4
  - @pnpm/normalize-registries@2.0.4

## 5.0.5

### Patch Changes

- Updated dependencies [8e76690f4]
- Updated dependencies [8e76690f4]
  - @pnpm/lockfile-file@4.1.0
  - @pnpm/types@7.3.0
  - @pnpm/modules-yaml@9.0.3
  - @pnpm/normalize-registries@2.0.3

## 5.0.4

### Patch Changes

- Updated dependencies [2dc5a7a4c]
  - @pnpm/lockfile-file@4.0.4

## 5.0.3

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/lockfile-file@4.0.3
  - @pnpm/modules-yaml@9.0.2
  - @pnpm/normalize-registries@2.0.2

## 5.0.2

### Patch Changes

- Updated dependencies [a1a03d145]
  - @pnpm/lockfile-file@4.0.2

## 5.0.1

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/lockfile-file@4.0.1
  - @pnpm/modules-yaml@9.0.1
  - @pnpm/normalize-registries@2.0.1

## 5.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [155e70597]
- Updated dependencies [9c2a878c3]
- Updated dependencies [8b66f26dc]
- Updated dependencies [f7750baed]
- Updated dependencies [78470a32d]
- Updated dependencies [9c2a878c3]
  - @pnpm/lockfile-file@4.0.0
  - @pnpm/modules-yaml@9.0.0
  - @pnpm/normalize-registries@2.0.0
  - @pnpm/types@7.0.0

## 4.0.16

### Patch Changes

- Updated dependencies [51e1456dd]
  - @pnpm/lockfile-file@3.2.1

## 4.0.15

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [9ad8c27bf]
  - @pnpm/lockfile-file@3.2.0
  - @pnpm/types@6.4.0
  - @pnpm/modules-yaml@8.0.6
  - @pnpm/normalize-registries@1.0.6

## 4.0.14

### Patch Changes

- Updated dependencies [af897c324]
  - @pnpm/lockfile-file@3.1.4

## 4.0.13

### Patch Changes

- Updated dependencies [1e4a3a17a]
  - @pnpm/lockfile-file@3.1.3

## 4.0.12

### Patch Changes

- Updated dependencies [fba715512]
  - @pnpm/lockfile-file@3.1.2

## 4.0.11

### Patch Changes

- @pnpm/lockfile-file@3.1.1

## 4.0.10

### Patch Changes

- Updated dependencies [3776b5a52]
  - @pnpm/lockfile-file@3.1.0

## 4.0.9

### Patch Changes

- Updated dependencies [dbcc6c96f]
- Updated dependencies [09492b7b4]
  - @pnpm/lockfile-file@3.0.18
  - @pnpm/modules-yaml@8.0.5

## 4.0.8

### Patch Changes

- Updated dependencies [aa6bc4f95]
  - @pnpm/lockfile-file@3.0.17

## 4.0.7

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/lockfile-file@3.0.16
  - @pnpm/modules-yaml@8.0.4
  - @pnpm/normalize-registries@1.0.5

## 4.0.6

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/lockfile-file@3.0.15
  - @pnpm/modules-yaml@8.0.3
  - @pnpm/normalize-registries@1.0.4

## 4.0.5

### Patch Changes

- @pnpm/lockfile-file@3.0.14

## 4.0.4

### Patch Changes

- Updated dependencies [9550b0505]
  - @pnpm/lockfile-file@3.0.13

## 4.0.3

### Patch Changes

- @pnpm/lockfile-file@3.0.12

## 4.0.2

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - @pnpm/modules-yaml@8.0.2

## 4.0.1

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/lockfile-file@3.0.11
  - @pnpm/modules-yaml@8.0.1
  - @pnpm/normalize-registries@1.0.3

## 4.0.0

### Major Changes

- 71a8c8ce3: `hoistedDependencies` is returned instead of `hoistedAliases`.

  `currentPublicHoistPattern` is returned instead of `shamefullyHoist`.

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/modules-yaml@8.0.0
  - @pnpm/lockfile-file@3.0.10
  - @pnpm/normalize-registries@1.0.2

## 3.0.0

### Major Changes

- 802d145fc: Remove `independent-leaves` support.

### Patch Changes

- Updated dependencies [3f73eaf0c]
- Updated dependencies [da091c711]
- Updated dependencies [802d145fc]
  - @pnpm/modules-yaml@7.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lockfile-file@3.0.9
  - @pnpm/normalize-registries@1.0.1

## 2.0.2-alpha.2

### Patch Changes

- @pnpm/lockfile-file@3.0.9-alpha.2

## 2.0.2-alpha.1

### Patch Changes

- Updated dependencies [3f73eaf0]
- Updated dependencies [da091c71]
  - @pnpm/modules-yaml@7.0.0-alpha.0
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.1
  - @pnpm/normalize-registries@1.0.1-alpha.0

## 2.0.2-alpha.0

### Patch Changes

- @pnpm/lockfile-file@3.0.9-alpha.0

## 2.0.1

### Patch Changes

- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/lockfile-file@3.0.8
  - @pnpm/modules-yaml@6.0.2
