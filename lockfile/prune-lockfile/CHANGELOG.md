# @pnpm/prune-lockfile

## 6.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- d381a60: Support for lockfile v5 is dropped. Use pnpm v8 to convert lockfile v5 to lockfile v6 [#7470](https://github.com/pnpm/pnpm/pull/7470).

### Minor Changes

- 086b69c: The checksum of the `.pnpmfile.cjs` is saved into the lockfile. If the pnpmfile gets modified, the lockfile is reanalyzed to apply the changes [#7662](https://github.com/pnpm/pnpm/pull/7662).
- 730929e: Add a field named `ignoredOptionalDependencies`. This is an array of strings. If an optional dependency has its name included in this array, it will be skipped.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [cdd8365]
- Updated dependencies [c692f80]
- Updated dependencies [89b396b]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [27a96a8]
- Updated dependencies [730929e]
- Updated dependencies [98a1266]
  - @pnpm/types@10.0.0
  - @pnpm/dependency-path@3.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/lockfile-types@6.0.0

## 5.0.9

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/lockfile-types@5.1.5
  - @pnpm/types@9.4.2
  - @pnpm/dependency-path@2.1.7

## 5.0.8

### Patch Changes

- Updated dependencies
  - @pnpm/lockfile-types@5.1.4
  - @pnpm/types@9.4.1
  - @pnpm/dependency-path@2.1.6

## 5.0.7

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/lockfile-types@5.1.3
  - @pnpm/dependency-path@2.1.5

## 5.0.6

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/lockfile-types@5.1.2
  - @pnpm/dependency-path@2.1.4

## 5.0.5

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/lockfile-types@5.1.1
  - @pnpm/dependency-path@2.1.3

## 5.0.4

### Patch Changes

- Updated dependencies [302ebffc5]
  - @pnpm/constants@7.1.1

## 5.0.3

### Patch Changes

- Updated dependencies [9c4ae87bd]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [9c4ae87bd]
  - @pnpm/lockfile-types@5.1.0
  - @pnpm/types@9.1.0
  - @pnpm/constants@7.1.0
  - @pnpm/dependency-path@2.1.2

## 5.0.2

### Patch Changes

- Updated dependencies [c0760128d]
  - @pnpm/dependency-path@2.1.1

## 5.0.1

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
  - @pnpm/dependency-path@2.1.0

## 5.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [c92936158]
- Updated dependencies [ca8f51e60]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [0e26acb0f]
  - @pnpm/lockfile-types@5.0.0
  - @pnpm/dependency-path@2.0.0
  - @pnpm/constants@7.0.0
  - @pnpm/types@9.0.0

## 4.0.24

### Patch Changes

- Updated dependencies [d89d7a078]
  - @pnpm/dependency-path@1.1.3

## 4.0.23

### Patch Changes

- Updated dependencies [9247f6781]
  - @pnpm/dependency-path@1.1.2

## 4.0.22

### Patch Changes

- Updated dependencies [0f6e95872]
  - @pnpm/dependency-path@1.1.1

## 4.0.21

### Patch Changes

- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
  - @pnpm/constants@6.2.0
  - @pnpm/dependency-path@1.1.0

## 4.0.20

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/lockfile-types@4.3.6
  - @pnpm/dependency-path@1.0.1

## 4.0.19

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/dependency-path@1.0.0

## 4.0.18

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - dependency-path@9.2.8
  - @pnpm/lockfile-types@4.3.5

## 4.0.17

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - dependency-path@9.2.7
  - @pnpm/lockfile-types@4.3.4

## 4.0.16

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - dependency-path@9.2.6
  - @pnpm/lockfile-types@4.3.3

## 4.0.15

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - dependency-path@9.2.5
  - @pnpm/lockfile-types@4.3.2

## 4.0.14

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270

## 4.0.13

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - dependency-path@9.2.4
  - @pnpm/lockfile-types@4.3.1

## 4.0.12

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-types@4.3.0

## 4.0.11

### Patch Changes

- dependency-path@9.2.3

## 4.0.10

### Patch Changes

- 5f643f23b: Update ramda to v0.28.

## 4.0.9

### Patch Changes

- Updated dependencies [fc581d371]
  - dependency-path@9.2.2

## 4.0.8

### Patch Changes

- Updated dependencies [d01c32355]
- Updated dependencies [8e5b77ef6]
- Updated dependencies [8e5b77ef6]
  - @pnpm/lockfile-types@4.2.0
  - @pnpm/types@8.4.0
  - dependency-path@9.2.1

## 4.0.7

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [c635f9fc1]
  - @pnpm/types@8.3.0
  - @pnpm/lockfile-types@4.1.0
  - dependency-path@9.2.0

## 4.0.6

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [725636a90]
  - @pnpm/types@8.2.0
  - dependency-path@9.1.4
  - @pnpm/lockfile-types@4.0.3

## 4.0.5

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - dependency-path@9.1.3
  - @pnpm/lockfile-types@4.0.2

## 4.0.4

### Patch Changes

- 190f0b331: Don't prune peer deps.

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

## 4.0.1

### Patch Changes

- Updated dependencies [0a70aedb1]
- Updated dependencies [1267e4eff]
  - dependency-path@9.1.0
  - @pnpm/constants@6.1.0

## 4.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [faf830b8f]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - dependency-path@9.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/lockfile-types@4.0.0

## 3.0.15

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-types@3.2.0
  - @pnpm/types@7.10.0
  - dependency-path@8.0.11

## 3.0.14

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - dependency-path@8.0.10
  - @pnpm/lockfile-types@3.1.5

## 3.0.13

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - dependency-path@8.0.9
  - @pnpm/lockfile-types@3.1.4

## 3.0.12

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - dependency-path@8.0.8
  - @pnpm/lockfile-types@3.1.3

## 3.0.11

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - dependency-path@8.0.7
  - @pnpm/lockfile-types@3.1.2

## 3.0.10

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - dependency-path@8.0.6
  - @pnpm/lockfile-types@3.1.1

## 3.0.9

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/lockfile-types@3.1.0
  - dependency-path@8.0.5

## 3.0.8

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - dependency-path@8.0.4

## 3.0.7

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - dependency-path@8.0.3

## 3.0.6

### Patch Changes

- Updated dependencies [6c418943c]
  - dependency-path@8.0.2

## 3.0.5

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - dependency-path@8.0.1

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

## 3.0.1

### Patch Changes

- Updated dependencies [9ceab68f0]
  - dependency-path@7.0.0

## 3.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [6871d74b2]
- Updated dependencies [97b986fbc]
- Updated dependencies [6871d74b2]
- Updated dependencies [e4efddbd2]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [f2bb5cbeb]
  - @pnpm/constants@5.0.0
  - dependency-path@6.0.0
  - @pnpm/lockfile-types@3.0.0
  - @pnpm/types@7.0.0

## 2.0.19

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [9ad8c27bf]
  - @pnpm/lockfile-types@2.2.0
  - @pnpm/types@6.4.0
  - dependency-path@5.1.1

## 2.0.18

### Patch Changes

- Updated dependencies [e27dcf0dc]
  - dependency-path@5.1.0

## 2.0.17

### Patch Changes

- Updated dependencies [39142e2ad]
  - dependency-path@5.0.6

## 2.0.16

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/lockfile-types@2.1.1
  - @pnpm/types@6.3.1
  - dependency-path@5.0.5

## 2.0.15

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
  - @pnpm/lockfile-types@2.1.0
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - dependency-path@5.0.4

## 2.0.14

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - dependency-path@5.0.3

## 2.0.13

### Patch Changes

- 873f08b04: Dev dependencies are not marked as prod dependencies if they are used as peer dependencies of prod dependencies.

## 2.0.12

### Patch Changes

- 7f25dad04: Don't remove optional dependencies of optional dependencies.

## 2.0.11

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - dependency-path@5.0.2

## 2.0.10

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - dependency-path@5.0.1

## 2.0.9

### Patch Changes

- Updated dependencies [41d92948b]
  - dependency-path@5.0.0

## 2.0.8

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [da091c711]
- Updated dependencies [6a8a97eee]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lockfile-types@2.0.1
  - dependency-path@4.0.7

## 2.0.8-alpha.2

### Patch Changes

- Updated dependencies [ca9f50844]
- Updated dependencies [6a8a97eee]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/lockfile-types@2.0.1-alpha.0

## 2.0.8-alpha.1

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - dependency-path@4.0.7-alpha.0

## 2.0.8-alpha.0

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
