# @pnpm/testing.temp-store

## 1100.1.11

### Patch Changes

- Updated dependencies [bae694f]
  - @pnpm/resolving.resolver-base@1100.5.0
  - @pnpm/store.controller-types@1100.1.6
  - @pnpm/installing.client@1100.2.10
  - @pnpm/store.controller@1102.0.2
  - @pnpm/testing.registry-mock@1100.0.7
  - @pnpm/store.index@1100.2.1

## 1100.1.10

### Patch Changes

- @pnpm/installing.client@1100.2.9
- @pnpm/store.controller@1102.0.1

## 1100.1.9

### Patch Changes

- Updated dependencies [61810aa]
- Updated dependencies [a31faa7]
  - @pnpm/store.index@1100.2.0
  - @pnpm/store.controller@1102.0.0
  - @pnpm/installing.client@1100.2.8
  - @pnpm/resolving.resolver-base@1100.4.2
  - @pnpm/store.controller-types@1100.1.5
  - @pnpm/testing.registry-mock@1100.0.6

## 1100.1.8

### Patch Changes

- @pnpm/store.controller@1101.0.13
- @pnpm/installing.client@1100.2.7
- @pnpm/testing.registry-mock@1100.0.5

## 1100.1.7

### Patch Changes

- @pnpm/installing.client@1100.2.6
- @pnpm/resolving.resolver-base@1100.4.1
- @pnpm/store.controller@1101.0.12
- @pnpm/store.controller-types@1100.1.4
- @pnpm/testing.registry-mock@1100.0.4

## 1100.1.6

### Patch Changes

- Updated dependencies [6d17b66]
  - @pnpm/resolving.resolver-base@1100.4.0
  - @pnpm/installing.client@1100.2.5
  - @pnpm/testing.registry-mock@1100.0.3
  - @pnpm/store.controller@1101.0.11
  - @pnpm/store.controller-types@1100.1.3

## 1100.1.5

### Patch Changes

- @pnpm/testing.registry-mock@1100.0.2
- @pnpm/installing.client@1100.2.4
- @pnpm/store.controller@1101.0.10

## 1100.1.4

### Patch Changes

- @pnpm/installing.client@1100.2.3
- @pnpm/resolving.resolver-base@1100.3.1
- @pnpm/store.controller@1101.0.9
- @pnpm/store.controller-types@1100.1.2

## 1100.1.3

### Patch Changes

- @pnpm/installing.client@1100.2.2
- @pnpm/store.controller@1101.0.8

## 1100.1.2

### Patch Changes

- @pnpm/installing.client@1100.2.1
- @pnpm/store.controller@1101.0.8

## 1100.1.1

### Patch Changes

- Updated dependencies [1627943]
  - @pnpm/installing.client@1100.2.0
  - @pnpm/resolving.resolver-base@1100.3.0
  - @pnpm/store.controller@1101.0.8
  - @pnpm/store.controller-types@1100.1.1

## 1100.1.0

### Minor Changes

- 31538bf: Restructured the `minimumReleaseAge` lockfile revalidation gate around a generic `ResolutionVerifier` interface. Each resolver may now export a sibling verifier factory (today: `createNpmResolutionVerifier`) that re-checks an already-resolved lockfile entry against its policies; the resolver chain returns the verifier list as `resolutionVerifiers` and the install side fans out across it. A `ResolutionVerifier` carries `verify` plus `policy` and `canTrustPastCheck` — the cache contract that lets repeat installs against an unchanged lockfile skip the per-package registry round trip entirely.

  Verification results are memoized in JSON Lines at `<cacheDir>/lockfile-verified.jsonl`: a stat-only fast path matches on lockfile size, mtime, and inode, falling back to a content hash when those drift (typical after a CI checkout). Every active verifier's policy contribution is merged into a single `policy` bag on the record; the gate runs in full whenever the lockfile changes, any verifier rejects the cached policy, or no record exists [#11687](https://github.com/pnpm/pnpm/issues/11687).

### Patch Changes

- Updated dependencies [4195766]
- Updated dependencies [31538bf]
  - @pnpm/resolving.resolver-base@1100.2.0
  - @pnpm/store.controller-types@1100.1.0
  - @pnpm/installing.client@1100.1.0
  - @pnpm/store.controller@1101.0.7

## 1100.0.16

### Patch Changes

- Updated dependencies [c2c2890]
  - @pnpm/store.controller-types@1100.0.7
  - @pnpm/installing.client@1100.0.15
  - @pnpm/store.controller@1101.0.6

## 1100.0.15

### Patch Changes

- @pnpm/installing.client@1100.0.14
- @pnpm/store.controller@1101.0.5

## 1100.0.14

### Patch Changes

- @pnpm/installing.client@1100.0.13
- @pnpm/store.controller@1101.0.5
- @pnpm/store.controller-types@1100.0.6

## 1100.0.13

### Patch Changes

- Updated dependencies [0c67cb5]
  - @pnpm/store.index@1100.1.0
  - @pnpm/installing.client@1100.0.12
  - @pnpm/store.controller@1101.0.4

## 1100.0.12

### Patch Changes

- @pnpm/installing.client@1100.0.11
- @pnpm/store.controller@1101.0.3

## 1100.0.11

### Patch Changes

- @pnpm/installing.client@1100.0.10
- @pnpm/store.controller@1101.0.3
- @pnpm/store.controller-types@1100.0.5

## 1100.0.10

### Patch Changes

- @pnpm/installing.client@1100.0.9
- @pnpm/store.controller@1101.0.2

## 1100.0.9

### Patch Changes

- @pnpm/installing.client@1100.0.8
- @pnpm/store.controller@1101.0.2

## 1100.0.8

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/store.controller-types@1100.0.4
  - @pnpm/installing.client@1100.0.7
  - @pnpm/store.controller@1101.0.2

## 1100.0.7

### Patch Changes

- @pnpm/store.controller@1101.0.1

## 1100.0.6

### Patch Changes

- @pnpm/installing.client@1100.0.6
- @pnpm/store.controller@1101.0.0

## 1100.0.5

### Patch Changes

- @pnpm/installing.client@1100.0.5
- @pnpm/store.controller@1101.0.0

## 1100.0.4

### Patch Changes

- @pnpm/installing.client@1100.0.4
- @pnpm/store.controller@1101.0.0
- @pnpm/store.controller-types@1100.0.3

## 1100.0.3

### Patch Changes

- @pnpm/installing.client@1100.0.3
- @pnpm/store.controller@1100.0.2
- @pnpm/store.controller-types@1100.0.2

## 1100.0.2

### Patch Changes

- @pnpm/installing.client@1100.0.2
- @pnpm/store.controller@1100.0.1

## 1100.0.1

### Patch Changes

- @pnpm/installing.client@1100.0.1
- @pnpm/store.controller@1100.0.1
- @pnpm/store.controller-types@1100.0.1

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Patch Changes

- Updated dependencies [facdd71]
- Updated dependencies [e2e0a32]
- Updated dependencies [5a0ed1d]
- Updated dependencies [491a84f]
- Updated dependencies [9eddabb]
- Updated dependencies [ba065f6]
- Updated dependencies [7d2fd48]
- Updated dependencies [9eddabb]
- Updated dependencies [56a59df]
- Updated dependencies [96704a1]
- Updated dependencies [10bc391]
- Updated dependencies [38b8e35]
- Updated dependencies [b7f0f21]
- Updated dependencies [2f98ec8]
- Updated dependencies [09bb8db]
- Updated dependencies [9d3f00b]
- Updated dependencies [98a0410]
  - @pnpm/store.controller-types@1005.0.0
  - @pnpm/store.controller@1005.0.0
  - @pnpm/installing.client@1002.0.0
  - @pnpm/store.index@1000.0.0

## 1000.0.23

### Patch Changes

- @pnpm/client@1001.1.4
- @pnpm/package-store@1004.0.0

## 1000.0.22

### Patch Changes

- Updated dependencies [7c1382f]
  - @pnpm/store-controller-types@1004.1.0
  - @pnpm/package-store@1004.0.0
  - @pnpm/client@1001.1.3

## 1000.0.21

### Patch Changes

- @pnpm/client@1001.1.2
- @pnpm/package-store@1003.0.0

## 1000.0.20

### Patch Changes

- @pnpm/package-store@1003.0.0
- @pnpm/client@1001.1.1

## 1000.0.19

### Patch Changes

- Updated dependencies [fb4da0c]
  - @pnpm/client@1001.1.0
  - @pnpm/package-store@1002.0.12

## 1000.0.18

### Patch Changes

- @pnpm/client@1001.0.7
- @pnpm/package-store@1002.0.11

## 1000.0.17

### Patch Changes

- @pnpm/client@1001.0.6
- @pnpm/package-store@1002.0.11

## 1000.0.16

### Patch Changes

- @pnpm/package-store@1002.0.11
- @pnpm/client@1001.0.5

## 1000.0.15

### Patch Changes

- @pnpm/client@1001.0.4
- @pnpm/package-store@1002.0.10
- @pnpm/store-controller-types@1004.0.2

## 1000.0.14

### Patch Changes

- @pnpm/client@1001.0.3
- @pnpm/package-store@1002.0.9

## 1000.0.13

### Patch Changes

- @pnpm/client@1001.0.2
- @pnpm/package-store@1002.0.9

## 1000.0.12

### Patch Changes

- @pnpm/client@1001.0.1
- @pnpm/package-store@1002.0.9

## 1000.0.11

### Patch Changes

- Updated dependencies [d1edf73]
- Updated dependencies [d1edf73]
- Updated dependencies [f91922c]
  - @pnpm/client@1001.0.0
  - @pnpm/package-store@1002.0.9
  - @pnpm/store-controller-types@1004.0.1

## 1000.0.10

### Patch Changes

- Updated dependencies [1a07b8f]
- Updated dependencies [1a07b8f]
  - @pnpm/store-controller-types@1004.0.0
  - @pnpm/client@1000.1.0
  - @pnpm/package-store@1002.0.8

## 1000.0.9

### Patch Changes

- @pnpm/package-store@1002.0.7
- @pnpm/client@1000.0.21

## 1000.0.8

### Patch Changes

- @pnpm/package-store@1002.0.6

## 1000.0.7

### Patch Changes

- @pnpm/client@1000.0.20
- @pnpm/package-store@1002.0.5
- @pnpm/store-controller-types@1003.0.3

## 1000.0.6

### Patch Changes

- Updated dependencies [509948d]
  - @pnpm/store-controller-types@1003.0.2
  - @pnpm/package-store@1002.0.4
  - @pnpm/client@1000.0.19

## 1000.0.5

### Patch Changes

- Updated dependencies [09cf46f]
- Updated dependencies [c24c66e]
  - @pnpm/package-store@1002.0.3
  - @pnpm/store-controller-types@1003.0.1
  - @pnpm/client@1000.0.18

## 1000.0.4

### Patch Changes

- @pnpm/client@1000.0.17
- @pnpm/package-store@1002.0.2

## 1000.0.3

### Patch Changes

- Updated dependencies [8a9f3a4]
- Updated dependencies [5b73df1]
- Updated dependencies [9c3dd03]
  - @pnpm/store-controller-types@1003.0.0
  - @pnpm/package-store@1002.0.2
  - @pnpm/client@1000.0.16

## 1000.0.2

### Patch Changes

- @pnpm/client@1000.0.15
- @pnpm/package-store@1002.0.1
- @pnpm/store-controller-types@1002.0.1

## 1000.0.1

### Patch Changes

- Updated dependencies [72cff38]
  - @pnpm/store-controller-types@1002.0.0
  - @pnpm/package-store@1002.0.0
  - @pnpm/client@1000.0.14

## 1000.0.0

### Major Changes

- a54d3ad: Initial release.

### Patch Changes

- Updated dependencies [a54d3ad]
  - @pnpm/package-store@1001.1.0
