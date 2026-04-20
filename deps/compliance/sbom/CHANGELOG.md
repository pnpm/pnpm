# @pnpm/deps.compliance.sbom

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
