# @pnpm/deps.compliance.sbom

## 1100.1.6

### Patch Changes

- Updated dependencies [a017bf3]
- Updated dependencies [6d17b66]
  - @pnpm/types@1101.3.0
  - @pnpm/resolving.resolver-base@1100.4.0
  - @pnpm/lockfile.detect-dep-types@1100.0.9
  - @pnpm/lockfile.types@1100.0.9
  - @pnpm/lockfile.utils@1100.0.11
  - @pnpm/lockfile.walker@1100.0.9
  - @pnpm/pkg-manifest.reader@1100.0.6
  - @pnpm/store.pkg-finder@1100.0.14

## 1100.1.5

### Patch Changes

- Updated dependencies [e55f4b5]
- Updated dependencies [35d2355]
  - @pnpm/lockfile.utils@1100.0.10
  - @pnpm/types@1101.2.0
  - @pnpm/lockfile.detect-dep-types@1100.0.8
  - @pnpm/lockfile.types@1100.0.8
  - @pnpm/lockfile.walker@1100.0.8
  - @pnpm/pkg-manifest.reader@1100.0.5
  - @pnpm/resolving.resolver-base@1100.3.1
  - @pnpm/store.pkg-finder@1100.0.13

## 1100.1.4

### Patch Changes

- @pnpm/store.pkg-finder@1100.0.12

## 1100.1.3

### Patch Changes

- Updated dependencies [1627943]
- Updated dependencies [64afc92]
  - @pnpm/resolving.resolver-base@1100.3.0
  - @pnpm/types@1101.1.1
  - @pnpm/lockfile.types@1100.0.7
  - @pnpm/lockfile.utils@1100.0.9
  - @pnpm/store.pkg-finder@1100.0.11
  - @pnpm/lockfile.detect-dep-types@1100.0.7
  - @pnpm/lockfile.walker@1100.0.7
  - @pnpm/pkg-manifest.reader@1100.0.4

## 1100.1.2

### Patch Changes

- Updated dependencies [4195766]
- Updated dependencies [31538bf]
  - @pnpm/resolving.resolver-base@1100.2.0
  - @pnpm/lockfile.types@1100.0.6
  - @pnpm/lockfile.utils@1100.0.8
  - @pnpm/store.pkg-finder@1100.0.10
  - @pnpm/lockfile.detect-dep-types@1100.0.6
  - @pnpm/lockfile.walker@1100.0.6

## 1100.1.1

### Patch Changes

- @pnpm/store.pkg-finder@1100.0.9

## 1100.1.0

### Minor Changes

- 87b4bac: Allow setting sbom spec version using `--sbom-spec-version` [#11389](https://github.com/pnpm/pnpm/pull/11389).

### Patch Changes

- Updated dependencies [b61e268]
  - @pnpm/types@1101.1.0
  - @pnpm/lockfile.detect-dep-types@1100.0.5
  - @pnpm/lockfile.types@1100.0.5
  - @pnpm/lockfile.utils@1100.0.7
  - @pnpm/lockfile.walker@1100.0.5
  - @pnpm/pkg-manifest.reader@1100.0.3
  - @pnpm/resolving.resolver-base@1100.1.3
  - @pnpm/store.pkg-finder@1100.0.8

## 1100.0.9

### Patch Changes

- Updated dependencies [0c67cb5]
  - @pnpm/store.index@1100.1.0
  - @pnpm/store.pkg-finder@1100.0.7

## 1100.0.8

### Patch Changes

- Updated dependencies [cfa271b]
  - @pnpm/lockfile.utils@1100.0.6

## 1100.0.7

### Patch Changes

- Updated dependencies [27425d7]
  - @pnpm/lockfile.types@1100.0.4
  - @pnpm/lockfile.utils@1100.0.5
  - @pnpm/resolving.resolver-base@1100.1.2
  - @pnpm/store.pkg-finder@1100.0.6
  - @pnpm/lockfile.detect-dep-types@1100.0.4
  - @pnpm/lockfile.walker@1100.0.4

## 1100.0.6

### Patch Changes

- Updated dependencies [184ce26]
- Updated dependencies [6b891a5]
  - @pnpm/resolving.resolver-base@1100.1.1
  - @pnpm/pkg-manifest.reader@1100.0.2
  - @pnpm/lockfile.utils@1100.0.4
  - @pnpm/store.pkg-finder@1100.0.5
  - @pnpm/lockfile.types@1100.0.3
  - @pnpm/lockfile.detect-dep-types@1100.0.3
  - @pnpm/lockfile.walker@1100.0.3

## 1100.0.5

### Patch Changes

- f9afe81: Populate download location for git-sourced dependencies in SBOM output. Previously `pnpm sbom` emitted `NOASSERTION` (SPDX) and omitted the distribution reference (CycloneDX) for git dependencies. Now emits the git URL with commit hash, e.g. `git+https://github.com/user/repo.git#commit`.

## 1100.0.4

### Patch Changes

- @pnpm/store.pkg-finder@1100.0.4
- @pnpm/lockfile.utils@1100.0.3

## 1100.0.3

### Patch Changes

- 61952c2: `pnpm sbom` now detects licenses declared via the deprecated `licenses` array in `package.json` (e.g. `busboy`, `streamsearch`, `limiter`) and falls back to scanning on-disk `LICENSE` files — mirroring the resolution logic of `pnpm licenses`. Previously these packages were reported as `NOASSERTION`. Shared license resolution (manifest parsing + LICENSE-file fallback) lives in the new `@pnpm/deps.compliance.license-resolver` package. When a manifest sets both `license` and `licenses`, the modern `license` field now takes precedence for both commands (previously `pnpm licenses` preferred `licenses`) [#11248](https://github.com/pnpm/pnpm/issues/11248).
- Updated dependencies [bcc88a1]
- Updated dependencies [61952c2]
  - @pnpm/store.pkg-finder@1100.0.3
  - @pnpm/deps.compliance.license-resolver@1100.0.0
  - @pnpm/lockfile.types@1100.0.2
  - @pnpm/lockfile.utils@1100.0.2
  - @pnpm/lockfile.detect-dep-types@1100.0.2
  - @pnpm/lockfile.walker@1100.0.2

## 1100.0.2

### Patch Changes

- @pnpm/store.pkg-finder@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/lockfile.detect-dep-types@1100.0.1
  - @pnpm/lockfile.types@1100.0.1
  - @pnpm/lockfile.utils@1100.0.1
  - @pnpm/lockfile.walker@1100.0.1
  - @pnpm/pkg-manifest.reader@1100.0.1
  - @pnpm/store.pkg-finder@1100.0.1

## 1000.0.0

### Minor Changes

- f92ac24: Added `pnpm sbom` command for generating Software Bill of Materials in CycloneDX 1.7 and SPDX 2.3 JSON formats [#9088](https://github.com/pnpm/pnpm/issues/9088).

### Patch Changes

- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [606f53e]
- Updated dependencies [491a84f]
- Updated dependencies [f92ac24]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [50fbeca]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [6f806be]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [38b8e35]
- Updated dependencies [394d88c]
- Updated dependencies [b7f0f21]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
  - @pnpm/types@1001.0.0
  - @pnpm/lockfile.types@1003.0.0
  - @pnpm/lockfile.utils@1004.0.0
  - @pnpm/pkg-manifest.reader@1001.0.0
  - @pnpm/lockfile.detect-dep-types@1002.0.0
  - @pnpm/lockfile.walker@1002.0.0
  - @pnpm/store.pkg-finder@1000.0.0
  - @pnpm/store.index@1000.0.0
