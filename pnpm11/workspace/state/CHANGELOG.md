# @pnpm/workspace.state

## 1100.0.24

### Patch Changes

- @pnpm/config.reader@1101.10.1

## 1100.0.23

### Patch Changes

- Updated dependencies [302a2f7]
- Updated dependencies [0474a9c]
  - @pnpm/config.reader@1101.10.0

## 1100.0.22

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [61810aa]
- Updated dependencies [681b593]
- Updated dependencies [a31faa7]
  - @pnpm/config.reader@1101.9.0
  - @pnpm/types@1101.3.2

## 1100.0.21

### Patch Changes

- Updated dependencies [bc9ed78]
- Updated dependencies [615c669]
  - @pnpm/config.reader@1101.8.0

## 1100.0.20

### Patch Changes

- 97e1982: Fix `pnpm install` ignoring `enableGlobalVirtualStore` toggle by including it in the workspace state settings check [#12142](https://github.com/pnpm/pnpm/issues/12142).
- Updated dependencies [822beb5]
- Updated dependencies [3537020]
- Updated dependencies [894ea6a]
- Updated dependencies [6b5d91a]
- Updated dependencies [027196b]
- Updated dependencies [1017c36]
- Updated dependencies [bf1b731]
  - @pnpm/config.reader@1101.7.0
  - @pnpm/types@1101.3.1

## 1100.0.19

### Patch Changes

- Updated dependencies [a017bf3]
  - @pnpm/config.reader@1101.6.0
  - @pnpm/types@1101.3.0

## 1100.0.18

### Patch Changes

- 37669c2: Avoid crashing when the workspace state cache is partially written or malformed.

## 1100.0.17

### Patch Changes

- Updated dependencies [a39a83d]
  - @pnpm/config.reader@1101.5.0

## 1100.0.16

### Patch Changes

- Updated dependencies [a23956e]
- Updated dependencies [35d2355]
  - @pnpm/config.reader@1101.4.1
  - @pnpm/types@1101.2.0

## 1100.0.15

### Patch Changes

- Updated dependencies [3b62f9d]
- Updated dependencies [212315d]
  - @pnpm/config.reader@1101.4.0

## 1100.0.14

### Patch Changes

- Updated dependencies [3687b0e]
- Updated dependencies [ced20cb]
- Updated dependencies [d1b340f]
- Updated dependencies [64afc92]
  - @pnpm/config.reader@1101.3.3
  - @pnpm/types@1101.1.1

## 1100.0.13

### Patch Changes

- Updated dependencies [020ac45]
- Updated dependencies [d3f8408]
- Updated dependencies [a62f959]
- Updated dependencies [ba2c884]
- Updated dependencies [8df408c]
  - @pnpm/config.reader@1101.3.2

## 1100.0.12

### Patch Changes

- @pnpm/config.reader@1101.3.1

## 1100.0.11

### Patch Changes

- Updated dependencies [b61e268]
- Updated dependencies [e1e29c1]
  - @pnpm/config.reader@1101.3.0
  - @pnpm/types@1101.1.0

## 1100.0.10

### Patch Changes

- Updated dependencies [e9e876c]
  - @pnpm/config.reader@1101.2.2

## 1100.0.9

### Patch Changes

- Updated dependencies [707a879]
  - @pnpm/config.reader@1101.2.1

## 1100.0.8

### Patch Changes

- ab6c42d: Treat `allowBuilds` as an install-state input and clear previously ignored builds when they are explicitly disallowed.
- Updated dependencies [8fdd9a9]
- Updated dependencies [5f34a8d]
- Updated dependencies [c969392]
- Updated dependencies [817b1b4]
- Updated dependencies [c969392]
- Updated dependencies [2de318b]
  - @pnpm/config.reader@1101.2.0

## 1100.0.7

### Patch Changes

- Updated dependencies [42a8f29]
  - @pnpm/config.reader@1101.1.4

## 1100.0.6

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/config.reader@1101.1.3

## 1100.0.5

### Patch Changes

- Updated dependencies [0fbcf74]
  - @pnpm/config.reader@1101.1.2

## 1100.0.4

### Patch Changes

- @pnpm/config.reader@1101.1.1

## 1100.0.3

### Patch Changes

- Updated dependencies [7d25bc1]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0

## 1100.0.2

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/config.reader@1101.0.0

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/config.reader@1100.0.1

## 1003.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- cc1b8e3: Fixed installation of config dependencies from private registries.

  Added support for object type in `configDependencies` when the tarball URL returned from package metadata differs from the computed URL [#10431](https://github.com/pnpm/pnpm/pull/10431).

### Patch Changes

- 03c502c: Fixed `optimisticRepeatInstall` skipping install when `overrides`, `packageExtensions`, `ignoredOptionalDependencies`, `patchedDependencies`, or `peersSuffixMaxLength` changed.
- Updated dependencies [7730a7f]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [3c72b6b]
- Updated dependencies [9f5c0e3]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [90bd3c3]
- Updated dependencies [1cc61e8]
- Updated dependencies [606f53e]
- Updated dependencies [c7203b9]
- Updated dependencies [bb17724]
- Updated dependencies [da2429d]
- Updated dependencies [1cc61e8]
- Updated dependencies [491a84f]
- Updated dependencies [f0ae1b9]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [075aa99]
- Updated dependencies [ae43ac7]
- Updated dependencies [ccec8e7]
- Updated dependencies [4158906]
- Updated dependencies [ac944ef]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [96704a1]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [2df8b71]
- Updated dependencies [ed1a7fe]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [5bf7768]
- Updated dependencies [ae43ac7]
- Updated dependencies [a5fdbf9]
- Updated dependencies [9d3f00b]
- Updated dependencies [efb48dc]
- Updated dependencies [9587dac]
- Updated dependencies [09a999a]
- Updated dependencies [559f903]
- Updated dependencies [3574905]
  - @pnpm/config.reader@1005.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/catalogs.types@1001.0.0

## 1002.0.7

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0
  - @pnpm/config@1004.4.2

## 1002.0.6

### Patch Changes

- Updated dependencies [9865167]
  - @pnpm/config@1004.4.1

## 1002.0.5

### Patch Changes

- Updated dependencies [fb4da0c]
  - @pnpm/config@1004.4.0

## 1002.0.4

### Patch Changes

- @pnpm/config@1004.3.1

## 1002.0.3

### Patch Changes

- Updated dependencies [38e2599]
- Updated dependencies [e792927]
  - @pnpm/config@1004.3.0
  - @pnpm/types@1000.8.0

## 1002.0.2

### Patch Changes

- @pnpm/config@1004.2.1

## 1002.0.1

### Patch Changes

- Updated dependencies [1a07b8f]
- Updated dependencies [6f7ac0f]
  - @pnpm/types@1000.7.0
  - @pnpm/config@1004.2.0

## 1002.0.0

### Major Changes

- cf630a8: Added the possibility to load multiple pnpmfiles. The `pnpmfile` setting can now accept a list of pnpmfile locations [#9702](https://github.com/pnpm/pnpm/pull/9702).

### Patch Changes

- Updated dependencies [623da6f]
- Updated dependencies [cf630a8]
  - @pnpm/config@1004.1.0

## 1001.1.22

### Patch Changes

- Updated dependencies [b217bbb]
- Updated dependencies [b0ead51]
- Updated dependencies [c8341cc]
- Updated dependencies [b0ead51]
- Updated dependencies [046af72]
  - @pnpm/config@1004.0.0

## 1001.1.21

### Patch Changes

- Updated dependencies [8d175c0]
  - @pnpm/config@1003.1.1

## 1001.1.20

### Patch Changes

- 09cf46f: Update `@pnpm/logger` in peer dependencies.
- Updated dependencies [b282bd1]
- Updated dependencies [fdb1d98]
- Updated dependencies [e4af08c]
- Updated dependencies [09cf46f]
- Updated dependencies [36d1448]
- Updated dependencies [9362b5f]
- Updated dependencies [5ec7255]
- Updated dependencies [6cf010c]
  - @pnpm/config@1003.1.0
  - @pnpm/types@1000.6.0

## 1001.1.19

### Patch Changes

- @pnpm/config@1003.0.1

## 1001.1.18

### Patch Changes

- Updated dependencies [56bb69b]
- Updated dependencies [8a9f3a4]
- Updated dependencies [9c3dd03]
- Updated dependencies [5b73df1]
  - @pnpm/config@1003.0.0
  - @pnpm/logger@1001.0.0
  - @pnpm/types@1000.5.0

## 1001.1.17

### Patch Changes

- @pnpm/config@1002.7.2

## 1001.1.16

### Patch Changes

- Updated dependencies [750ae7d]
- Updated dependencies [5679712]
- Updated dependencies [01f2bcf]
  - @pnpm/types@1000.4.0
  - @pnpm/config@1002.7.1

## 1001.1.15

### Patch Changes

- Updated dependencies [e57f1df]
  - @pnpm/config@1002.7.0

## 1001.1.14

### Patch Changes

- Updated dependencies [9bcca9f]
- Updated dependencies [5b35dff]
- Updated dependencies [9bcca9f]
- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
  - @pnpm/config@1002.6.0
  - @pnpm/types@1000.3.0

## 1001.1.13

### Patch Changes

- Updated dependencies [936430a]
  - @pnpm/config@1002.5.4

## 1001.1.12

### Patch Changes

- 9904675: `@pnpm/logger` should be a peer dependency.

## 1001.1.11

### Patch Changes

- Updated dependencies [6e4459c]
  - @pnpm/config@1002.5.3

## 1001.1.10

### Patch Changes

- @pnpm/config@1002.5.2

## 1001.1.9

### Patch Changes

- Updated dependencies [c3aa4d8]
  - @pnpm/config@1002.5.1

## 1001.1.8

### Patch Changes

- Updated dependencies [a5e4965]
- Updated dependencies [d965748]
  - @pnpm/types@1000.2.1
  - @pnpm/config@1002.5.0

## 1001.1.7

### Patch Changes

- Updated dependencies [1c2eb8c]
  - @pnpm/config@1002.4.1

## 1001.1.6

### Patch Changes

- Updated dependencies [8fcc221]
- Updated dependencies [e32b1a2]
- Updated dependencies [8fcc221]
  - @pnpm/config@1002.4.0
  - @pnpm/types@1000.2.0

## 1001.1.5

### Patch Changes

- Updated dependencies [fee898f]
  - @pnpm/config@1002.3.1

## 1001.1.4

### Patch Changes

- Updated dependencies [f6006f2]
  - @pnpm/config@1002.3.0

## 1001.1.3

### Patch Changes

- @pnpm/config@1002.2.1

## 1001.1.2

### Patch Changes

- Updated dependencies [b562deb]
- Updated dependencies [f3ffaed]
- Updated dependencies [c96eb2b]
  - @pnpm/types@1000.1.1
  - @pnpm/config@1002.2.0

## 1001.1.1

### Patch Changes

- @pnpm/config@1002.1.2

## 1001.1.0

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

### Patch Changes

- Updated dependencies [9591a18]
- Updated dependencies [1f5169f]
  - @pnpm/types@1000.1.0
  - @pnpm/config@1002.1.1

## 1001.0.2

### Patch Changes

- Updated dependencies [f90a94b]
- Updated dependencies [f891288]
  - @pnpm/config@1002.1.0

## 1001.0.1

### Patch Changes

- Updated dependencies [878ea8c]
  - @pnpm/config@1002.0.0

## 1001.0.0

### Major Changes

- d47c426: On repeat install perform a fast check if `node_modules` is up to date [#8838](https://github.com/pnpm/pnpm/pull/8838).

### Patch Changes

- Updated dependencies [ac5b9d8]
- Updated dependencies [6483b64]
  - @pnpm/config@1001.0.0

## 1.0.0

### Major Changes

- 19d5b51: Initial Release
