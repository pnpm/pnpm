# @pnpm/lockfile.preferred-versions

## 1100.0.4

### Patch Changes

- @pnpm/lockfile.utils@1100.0.3

## 1100.0.3

### Patch Changes

- Updated dependencies [72c1e05]
  - @pnpm/resolving.resolver-base@1100.1.0
  - @pnpm/lockfile.utils@1100.0.2

## 1100.0.2

### Patch Changes

- Updated dependencies [ff7733c]
  - @pnpm/pkg-manifest.utils@1100.1.0

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/lockfile.utils@1100.0.1
  - @pnpm/pkg-manifest.utils@1100.0.1
  - @pnpm/resolving.resolver-base@1100.0.1

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Patch Changes

- 9b0a460: Fixed a resolution bug that could cause `pnpm dedupe --check` to fail unexpectedly.

  When adding new dependencies to `package.json`, pnpm generally reuses existing versions in the `pnpm-lock.yaml` if they are satisfied by the version range specifier. There was an edge case where pnpm would instead resolve to a newly released version of a dependency. This is particularly problematic for `pnpm dedupe --check`, since a new version of a dependency published to the NPM registry could cause this check to suddenly fail. For details of this bug, see [#10626](https://github.com/pnpm/pnpm/issues/10626). This bug has been fixed.

  The fix necessitated a behavioral change: In some cases, pnpm was previously able to automatically dedupe a newly used dependency deep in the dependency graph without needing to run `pnpm dedupe`. This behavior was supported by the non-determinism that is now corrected. We believe fixing this non-determinism is more important than preserving an automatic dedupe heuristic that didn't handle all cases. The `pnpm dedupe` command can still be used to clean up dependencies that aren't automatically deduped on `pnpm install`.

- Updated dependencies [facdd71]
- Updated dependencies [9b0a460]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [efb48dc]
- Updated dependencies [491a84f]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [50fbeca]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [8ffb1a7]
- Updated dependencies [cee1f58]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [38b8e35]
- Updated dependencies [394d88c]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [9d3f00b]
- Updated dependencies [efb48dc]
- Updated dependencies [efb48dc]
  - @pnpm/resolving.resolver-base@1006.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/lockfile.utils@1004.0.0
  - @pnpm/pkg-manifest.utils@1002.0.0

## 1000.0.22

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0
  - @pnpm/resolver-base@1005.1.0
  - @pnpm/lockfile.utils@1003.0.3
  - @pnpm/manifest-utils@1001.0.6

## 1000.0.21

### Patch Changes

- @pnpm/lockfile.utils@1003.0.2

## 1000.0.20

### Patch Changes

- @pnpm/manifest-utils@1001.0.5

## 1000.0.19

### Patch Changes

- Updated dependencies [e792927]
  - @pnpm/types@1000.8.0
  - @pnpm/lockfile.utils@1003.0.1
  - @pnpm/manifest-utils@1001.0.4
  - @pnpm/resolver-base@1005.0.1

## 1000.0.18

### Patch Changes

- Updated dependencies [d1edf73]
- Updated dependencies [86b33e9]
- Updated dependencies [d1edf73]
- Updated dependencies [f91922c]
  - @pnpm/lockfile.utils@1003.0.0
  - @pnpm/resolver-base@1005.0.0
  - @pnpm/manifest-utils@1001.0.3

## 1000.0.17

### Patch Changes

- Updated dependencies [1a07b8f]
- Updated dependencies [2e85f29]
- Updated dependencies [1a07b8f]
  - @pnpm/types@1000.7.0
  - @pnpm/lockfile.utils@1002.1.0
  - @pnpm/resolver-base@1004.1.0
  - @pnpm/manifest-utils@1001.0.2

## 1000.0.16

### Patch Changes

- @pnpm/lockfile.utils@1002.0.1

## 1000.0.15

### Patch Changes

- Updated dependencies [540986f]
  - @pnpm/lockfile.utils@1002.0.0

## 1000.0.14

### Patch Changes

- Updated dependencies [2721291]
- Updated dependencies [6acf819]
  - @pnpm/resolver-base@1004.0.0
  - @pnpm/lockfile.utils@1001.0.12

## 1000.0.13

### Patch Changes

- Updated dependencies [5ec7255]
  - @pnpm/types@1000.6.0
  - @pnpm/manifest-utils@1001.0.1
  - @pnpm/lockfile.utils@1001.0.11
  - @pnpm/resolver-base@1003.0.1

## 1000.0.12

### Patch Changes

- Updated dependencies [8a9f3a4]
- Updated dependencies [5b73df1]
- Updated dependencies [5b73df1]
- Updated dependencies [9c3dd03]
- Updated dependencies [5b73df1]
  - @pnpm/resolver-base@1003.0.0
  - @pnpm/manifest-utils@1001.0.0
  - @pnpm/types@1000.5.0
  - @pnpm/lockfile.utils@1001.0.10

## 1000.0.11

### Patch Changes

- Updated dependencies [81f441c]
  - @pnpm/resolver-base@1002.0.0
  - @pnpm/lockfile.utils@1001.0.9

## 1000.0.10

### Patch Changes

- Updated dependencies [750ae7d]
- Updated dependencies [72cff38]
  - @pnpm/types@1000.4.0
  - @pnpm/resolver-base@1001.0.0
  - @pnpm/lockfile.utils@1001.0.8
  - @pnpm/manifest-utils@1000.0.8

## 1000.0.9

### Patch Changes

- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
  - @pnpm/types@1000.3.0
  - @pnpm/lockfile.utils@1001.0.7
  - @pnpm/manifest-utils@1000.0.7
  - @pnpm/resolver-base@1000.2.1

## 1000.0.8

### Patch Changes

- Updated dependencies [3d52365]
  - @pnpm/resolver-base@1000.2.0
  - @pnpm/lockfile.utils@1001.0.6

## 1000.0.7

### Patch Changes

- @pnpm/lockfile.utils@1001.0.5

## 1000.0.6

### Patch Changes

- Updated dependencies [a5e4965]
  - @pnpm/types@1000.2.1
  - @pnpm/lockfile.utils@1001.0.4
  - @pnpm/manifest-utils@1000.0.6
  - @pnpm/resolver-base@1000.1.4

## 1000.0.5

### Patch Changes

- Updated dependencies [8fcc221]
  - @pnpm/types@1000.2.0
  - @pnpm/lockfile.utils@1001.0.3
  - @pnpm/manifest-utils@1000.0.5
  - @pnpm/resolver-base@1000.1.3

## 1000.0.4

### Patch Changes

- Updated dependencies [b562deb]
  - @pnpm/types@1000.1.1
  - @pnpm/lockfile.utils@1001.0.2
  - @pnpm/manifest-utils@1000.0.4
  - @pnpm/resolver-base@1000.1.2

## 1000.0.3

### Patch Changes

- Updated dependencies [9591a18]
  - @pnpm/types@1000.1.0
  - @pnpm/lockfile.utils@1001.0.1
  - @pnpm/manifest-utils@1000.0.3
  - @pnpm/resolver-base@1000.1.1

## 1000.0.2

### Patch Changes

- @pnpm/manifest-utils@1000.0.2

## 1000.0.1

### Patch Changes

- Updated dependencies [6483b64]
- Updated dependencies [a76da0c]
  - @pnpm/resolver-base@1000.1.0
  - @pnpm/lockfile.utils@1001.0.0
  - @pnpm/manifest-utils@1000.0.1

## 1.0.15

### Patch Changes

- @pnpm/lockfile.utils@1.0.5
- @pnpm/manifest-utils@6.0.10

## 1.0.14

### Patch Changes

- @pnpm/lockfile.utils@1.0.4

## 1.0.13

### Patch Changes

- @pnpm/manifest-utils@6.0.9

## 1.0.12

### Patch Changes

- Updated dependencies [d500d9f]
  - @pnpm/types@12.2.0
  - @pnpm/lockfile.utils@1.0.3
  - @pnpm/manifest-utils@6.0.8
  - @pnpm/resolver-base@13.0.4

## 1.0.11

### Patch Changes

- Updated dependencies [7ee59a1]
  - @pnpm/types@12.1.0
  - @pnpm/lockfile.utils@1.0.2
  - @pnpm/manifest-utils@6.0.7
  - @pnpm/resolver-base@13.0.3

## 1.0.10

### Patch Changes

- Updated dependencies [cb006df]
  - @pnpm/types@12.0.0
  - @pnpm/lockfile.utils@1.0.1
  - @pnpm/manifest-utils@6.0.6
  - @pnpm/resolver-base@13.0.2

## 1.0.9

### Patch Changes

- Updated dependencies [c5ef9b0]
  - @pnpm/lockfile.utils@1.0.0

## 1.0.8

### Patch Changes

- Updated dependencies [0ef168b]
  - @pnpm/types@11.1.0
  - @pnpm/lockfile-utils@11.0.4
  - @pnpm/manifest-utils@6.0.5
  - @pnpm/resolver-base@13.0.1

## 1.0.7

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/resolver-base@13.0.0
  - @pnpm/types@11.0.0
  - @pnpm/lockfile-utils@11.0.3
  - @pnpm/manifest-utils@6.0.4

## 1.0.6

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/lockfile-utils@11.0.2
  - @pnpm/manifest-utils@6.0.3
  - @pnpm/resolver-base@12.0.2

## 1.0.5

### Patch Changes

- @pnpm/lockfile-utils@11.0.1

## 1.0.4

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-utils@11.0.0
  - @pnpm/manifest-utils@6.0.2
  - @pnpm/resolver-base@12.0.1

## 1.0.3

### Patch Changes

- @pnpm/manifest-utils@6.0.1

## 1.0.2

### Patch Changes

- Updated dependencies [7a0536e]
  - @pnpm/lockfile-utils@10.1.1

## 1.0.1

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/lockfile-utils@10.1.0

## 1.0.0

### Major Changes

- 8eddd21: Initial release.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [cdd8365]
- Updated dependencies [43cdd87]
- Updated dependencies [d381a60]
- Updated dependencies [b13d2dc]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/lockfile-utils@10.0.0
  - @pnpm/manifest-utils@6.0.0
  - @pnpm/resolver-base@12.0.0
