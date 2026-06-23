# @pnpm/resolving.bun-resolver

## 1102.0.2

### Patch Changes

- 0ec878d: Removing a runtime dependency now removes the matching `devEngines.runtime` or `engines.runtime` entry that was materialized from it. Blank runtime selectors are normalized to `latest`.
- Updated dependencies [bae694f]
- Updated dependencies [fa7004b]
- Updated dependencies [852d537]
  - @pnpm/resolving.npm-resolver@1102.1.0
  - @pnpm/resolving.resolver-base@1100.5.0
  - @pnpm/fetching.fetcher-base@1100.2.0
  - @pnpm/error@1100.0.1
  - @pnpm/fetching.binary-fetcher@1102.0.1
  - @pnpm/crypto.shasums-file@1100.1.2
  - @pnpm/worker@1100.2.2

## 1102.0.1

### Patch Changes

- Updated dependencies [29ab905]
  - @pnpm/resolving.npm-resolver@1102.0.1
  - @pnpm/worker@1100.2.1

## 1102.0.0

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [61810aa]
- Updated dependencies [681b593]
- Updated dependencies [1310ab5]
- Updated dependencies [a31faa7]
  - @pnpm/resolving.npm-resolver@1102.0.0
  - @pnpm/worker@1100.2.0
  - @pnpm/fetching.types@1100.0.2
  - @pnpm/types@1101.3.2
  - @pnpm/fetching.binary-fetcher@1102.0.0
  - @pnpm/crypto.shasums-file@1100.1.1
  - @pnpm/fetching.fetcher-base@1100.1.9
  - @pnpm/resolving.resolver-base@1100.4.2

## 1101.1.7

### Patch Changes

- @pnpm/resolving.npm-resolver@1101.5.2
- @pnpm/worker@1100.1.11

## 1101.1.6

### Patch Changes

- Updated dependencies [089484a]
- Updated dependencies [bf1b731]
- Updated dependencies [3d50680]
  - @pnpm/worker@1100.1.10
  - @pnpm/types@1101.3.1
  - @pnpm/crypto.shasums-file@1100.1.0
  - @pnpm/fetching.fetcher-base@1100.1.8
  - @pnpm/resolving.npm-resolver@1101.5.1
  - @pnpm/resolving.resolver-base@1100.4.1
  - @pnpm/fetching.binary-fetcher@1101.0.10

## 1101.1.5

### Patch Changes

- Updated dependencies [3b76b8e]
- Updated dependencies [a017bf3]
- Updated dependencies [722b9cd]
- Updated dependencies [6d17b66]
  - @pnpm/worker@1100.1.9
  - @pnpm/types@1101.3.0
  - @pnpm/resolving.npm-resolver@1101.5.0
  - @pnpm/resolving.resolver-base@1100.4.0
  - @pnpm/fetching.fetcher-base@1100.1.7
  - @pnpm/fetching.binary-fetcher@1101.0.9

## 1101.1.4

### Patch Changes

- Updated dependencies [6235428]
- Updated dependencies [1e9ab29]
  - @pnpm/resolving.npm-resolver@1101.4.0

## 1101.1.3

### Patch Changes

- Updated dependencies [aa6149d]
- Updated dependencies [35d2355]
- Updated dependencies [0721d64]
  - @pnpm/worker@1100.1.8
  - @pnpm/types@1101.2.0
  - @pnpm/resolving.npm-resolver@1101.3.3
  - @pnpm/fetching.fetcher-base@1100.1.6
  - @pnpm/resolving.resolver-base@1100.3.1
  - @pnpm/fetching.binary-fetcher@1101.0.8

## 1101.1.2

### Patch Changes

- Updated dependencies [212315d]
  - @pnpm/resolving.npm-resolver@1101.3.2

## 1101.1.1

### Patch Changes

- @pnpm/resolving.npm-resolver@1101.3.1

## 1101.1.0

### Minor Changes

- 1627943: `pnpm outdated` and `pnpm update --interactive` now report Node.js, Deno, and Bun runtimes installed as project dependencies (`runtime:` specifiers). Previously these were silently skipped because the npm specifier parser did not understand the `runtime:` protocol, so runtime versions never appeared in the outdated table or the interactive update picker.

  Internally, the outdated check is now resolver-driven: `@pnpm/resolving.resolver-base` defines a `ResolveLatestFunction` shape (with `LatestQuery` input — `{ wantedDependency, compatible? }` — and `LatestInfo` result — `{ latestManifest? }`), and every protocol resolver (npm, jsr, named-registry, git, tarball, local, node/bun/deno runtimes) exports its own `resolveLatest*` function alongside its `resolve*`. `@pnpm/resolving.default-resolver` composes them into a single dispatcher, exposed through `@pnpm/installing.client` as `createResolver(...).resolveLatest`.

  Each resolver decides whether it owns the dep and what "latest" means for its protocol; the outdated command derives `current` / `wanted` display values from the lockfile snapshot (`pkgSnapshot.version` for semver protocols, raw ref for URL-shaped ones) and uses raw ref equality for the "lockfile changed" check, so protocol knowledge stays inside each resolver instead of the command.

### Patch Changes

- Updated dependencies [3a54205]
- Updated dependencies [1627943]
- Updated dependencies [64afc92]
  - @pnpm/resolving.npm-resolver@1101.3.0
  - @pnpm/resolving.resolver-base@1100.3.0
  - @pnpm/types@1101.1.1
  - @pnpm/fetching.fetcher-base@1100.1.5
  - @pnpm/worker@1100.1.7
  - @pnpm/fetching.binary-fetcher@1101.0.7

## 1101.0.7

### Patch Changes

- Updated dependencies [963861c]
- Updated dependencies [4195766]
- Updated dependencies [31538bf]
  - @pnpm/resolving.npm-resolver@1101.2.0
  - @pnpm/resolving.resolver-base@1100.2.0
  - @pnpm/fetching.fetcher-base@1100.1.4
  - @pnpm/fetching.binary-fetcher@1101.0.6
  - @pnpm/worker@1100.1.6

## 1101.0.6

### Patch Changes

- Updated dependencies [50b33c1]
- Updated dependencies [e526f89]
- Updated dependencies [c2c2890]
  - @pnpm/resolving.npm-resolver@1101.1.1
  - @pnpm/worker@1100.1.5

## 1101.0.5

### Patch Changes

- Updated dependencies [b61e268]
  - @pnpm/resolving.npm-resolver@1101.1.0
  - @pnpm/types@1101.1.0
  - @pnpm/fetching.fetcher-base@1100.1.3
  - @pnpm/resolving.resolver-base@1100.1.3
  - @pnpm/worker@1100.1.4
  - @pnpm/fetching.binary-fetcher@1101.0.5

## 1101.0.4

### Patch Changes

- Updated dependencies [15e9e35]
  - @pnpm/resolving.npm-resolver@1101.0.3
  - @pnpm/fetching.binary-fetcher@1101.0.4
  - @pnpm/worker@1100.1.3

## 1101.0.3

### Patch Changes

- Updated dependencies [27425d7]
  - @pnpm/resolving.resolver-base@1100.1.2
  - @pnpm/fetching.fetcher-base@1100.1.2
  - @pnpm/resolving.npm-resolver@1101.0.2
  - @pnpm/fetching.binary-fetcher@1101.0.3
  - @pnpm/worker@1100.1.2

## 1101.0.2

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/resolving.resolver-base@1100.1.1
  - @pnpm/resolving.npm-resolver@1101.0.1
  - @pnpm/fetching.fetcher-base@1100.1.1
  - @pnpm/fetching.types@1100.0.1
  - @pnpm/worker@1100.1.1
  - @pnpm/fetching.binary-fetcher@1101.0.2
  - @pnpm/crypto.shasums-file@1100.0.1

## 1101.0.1

### Patch Changes

- Updated dependencies [dd23d19]
  - @pnpm/fetching.binary-fetcher@1101.0.1

## 1101.0.0

### Patch Changes

- Updated dependencies [421317c]
  - @pnpm/fetching.binary-fetcher@1101.0.0
  - @pnpm/fetching.fetcher-base@1100.1.0
  - @pnpm/worker@1100.1.0
  - @pnpm/resolving.npm-resolver@1101.0.0

## 1100.0.2

### Patch Changes

- Updated dependencies [72c1e05]
- Updated dependencies [9e0833c]
  - @pnpm/resolving.resolver-base@1100.1.0
  - @pnpm/resolving.npm-resolver@1100.1.0
  - @pnpm/fetching.fetcher-base@1100.0.2
  - @pnpm/fetching.binary-fetcher@1100.0.2
  - @pnpm/worker@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/fetching.fetcher-base@1100.0.1
  - @pnpm/resolving.npm-resolver@1100.0.1
  - @pnpm/resolving.resolver-base@1100.0.1
  - @pnpm/worker@1100.0.1
  - @pnpm/fetching.binary-fetcher@1100.0.1

## 1003.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Patch Changes

- Updated dependencies [facdd71]
- Updated dependencies [e2e0a32]
- Updated dependencies [9b0a460]
- Updated dependencies [a297ebc]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [7cec347]
- Updated dependencies [3bf5e21]
- Updated dependencies [831f574]
- Updated dependencies [0e9c559]
- Updated dependencies [19f36cf]
- Updated dependencies [491a84f]
- Updated dependencies [260899d]
- Updated dependencies [61cad0c]
- Updated dependencies [50fbeca]
- Updated dependencies [19f36cf]
- Updated dependencies [143ca78]
- Updated dependencies [ba065f6]
- Updated dependencies [3bf5e21]
- Updated dependencies [6f361aa]
- Updated dependencies [0625e20]
- Updated dependencies [938ea1f]
- Updated dependencies [2cb0657]
- Updated dependencies [bb8baa7]
- Updated dependencies [ee9fe58]
- Updated dependencies [7d2fd48]
- Updated dependencies [144ce0e]
- Updated dependencies [efb48dc]
- Updated dependencies [56a59df]
- Updated dependencies [780af09]
- Updated dependencies [96704a1]
- Updated dependencies [50fbeca]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [6c480a4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [4893853]
- Updated dependencies [10bc391]
- Updated dependencies [ba70035]
- Updated dependencies [3585d9a]
- Updated dependencies [38b8e35]
- Updated dependencies [b7f0f21]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [9d3f00b]
- Updated dependencies [6557dc0]
- Updated dependencies [98a0410]
- Updated dependencies [efb48dc]
  - @pnpm/resolving.resolver-base@1006.0.0
  - @pnpm/resolving.npm-resolver@1005.0.0
  - @pnpm/worker@1001.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/fetching.binary-fetcher@1003.0.0
  - @pnpm/fetching.types@1001.0.0
  - @pnpm/fetching.fetcher-base@1002.0.0
  - @pnpm/crypto.shasums-file@1002.0.0
  - @pnpm/error@1001.0.0

## 1002.0.1

### Patch Changes

- Updated dependencies [6c3dcb8]
  - @pnpm/npm-resolver@1004.4.1

## 1002.0.0

### Patch Changes

- Updated dependencies [8993f68]
- Updated dependencies [7c1382f]
- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/worker@1000.3.0
  - @pnpm/types@1000.9.0
  - @pnpm/resolver-base@1005.1.0
  - @pnpm/npm-resolver@1004.4.0
  - @pnpm/fetching.binary-fetcher@1002.0.0
  - @pnpm/fetcher-base@1001.0.2
  - @pnpm/node.fetcher@1001.0.8

## 1001.0.1

### Patch Changes

- @pnpm/node.fetcher@1001.0.7

## 1001.0.0

### Patch Changes

- Updated dependencies [06d2160]
  - @pnpm/worker@1000.2.0
  - @pnpm/fetching.binary-fetcher@1001.0.0
  - @pnpm/node.fetcher@1001.0.6

## 1000.0.7

### Patch Changes

- Updated dependencies [fb4da0c]
  - @pnpm/npm-resolver@1004.3.0
  - @pnpm/worker@1000.1.14
  - @pnpm/node.fetcher@1001.0.5
  - @pnpm/crypto.shasums-file@1001.0.2

## 1000.0.6

### Patch Changes

- Updated dependencies [baf8bf6]
- Updated dependencies [702ddb9]
  - @pnpm/npm-resolver@1004.2.3

## 1000.0.5

### Patch Changes

- Updated dependencies [121b44e]
- Updated dependencies [02f8b69]
  - @pnpm/npm-resolver@1004.2.2

## 1000.0.4

### Patch Changes

- Updated dependencies [6365bc4]
  - @pnpm/constants@1001.3.1
  - @pnpm/error@1000.0.5
  - @pnpm/npm-resolver@1004.2.1
  - @pnpm/node.fetcher@1001.0.4
  - @pnpm/crypto.shasums-file@1001.0.1
  - @pnpm/fetching.binary-fetcher@1000.0.3
  - @pnpm/worker@1000.1.13

## 1000.0.3

### Patch Changes

- Updated dependencies [38e2599]
- Updated dependencies [e792927]
  - @pnpm/npm-resolver@1004.2.0
  - @pnpm/types@1000.8.0
  - @pnpm/fetcher-base@1001.0.1
  - @pnpm/resolver-base@1005.0.1
  - @pnpm/worker@1000.1.12
  - @pnpm/node.fetcher@1001.0.3
  - @pnpm/fetching.binary-fetcher@1000.0.2

## 1000.0.2

### Patch Changes

- @pnpm/node.fetcher@1001.0.2

## 1000.0.1

### Patch Changes

- 2b0d35f: `@pnpm/worker` should always be a peer dependency.
- Updated dependencies [2b0d35f]
  - @pnpm/fetching.binary-fetcher@1000.0.1
  - @pnpm/node.fetcher@1001.0.1

## 1000.0.0

### Major Changes

- 86b33e9: Added support for installing Bun runtime.

### Patch Changes

- Updated dependencies [d1edf73]
- Updated dependencies [d1edf73]
- Updated dependencies [86b33e9]
- Updated dependencies [d1edf73]
- Updated dependencies [86b33e9]
- Updated dependencies [d1edf73]
- Updated dependencies [f91922c]
  - @pnpm/constants@1001.3.0
  - @pnpm/node.fetcher@1001.0.0
  - @pnpm/fetcher-base@1001.0.0
  - @pnpm/resolver-base@1005.0.0
  - @pnpm/fetching.binary-fetcher@1000.0.0
  - @pnpm/crypto.shasums-file@1001.0.0
  - @pnpm/error@1000.0.4
  - @pnpm/npm-resolver@1004.1.3
  - @pnpm/worker@1000.1.11
