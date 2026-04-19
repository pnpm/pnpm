# @pnpm/deps.compliance.commands

## 1101.0.1

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/cli.utils@1101.0.0
  - @pnpm/config.reader@1101.0.0
  - @pnpm/installing.commands@1100.1.0
  - @pnpm/workspace.project-manifest-reader@1100.0.2
  - @pnpm/deps.compliance.license-scanner@1100.0.2
  - @pnpm/deps.compliance.sbom@1100.0.2

## 1101.0.0

### Major Changes

- ff28085: `pnpm audit` now calls npm's `/-/npm/v1/security/advisories/bulk` endpoint. The legacy `/-/npm/v1/security/audits{,/quick}` endpoints have been retired by the registry, so the legacy request/response contract is no longer supported.

  The bulk endpoint does not return CVE identifiers. CVE-based filtering has been replaced with GitHub advisory ID (GHSA) filtering:

  - `auditConfig.ignoreCves` → `auditConfig.ignoreGhsas` (the previous key is no longer recognized)
  - `pnpm audit --ignore <id>` / `pnpm audit --ignore-unfixable` now read and write GHSAs instead of CVEs
  - GHSAs are derived from each advisory's `url` (`https://github.com/advisories/GHSA-xxxx-xxxx-xxxx`)

  To migrate: replace each `CVE-YYYY-NNNNN` entry in your `auditConfig.ignoreCves` with the corresponding `GHSA-xxxx-xxxx-xxxx` value (visible in the `More info` column of `pnpm audit` output) and move it under `auditConfig.ignoreGhsas`.

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/deps.compliance.audit@1101.0.0
  - @pnpm/types@1101.0.0
  - @pnpm/cli.meta@1100.0.1
  - @pnpm/cli.utils@1100.0.1
  - @pnpm/config.reader@1100.0.1
  - @pnpm/config.writer@1100.0.1
  - @pnpm/deps.compliance.license-scanner@1100.0.1
  - @pnpm/deps.compliance.sbom@1100.0.1
  - @pnpm/installing.commands@1100.0.1
  - @pnpm/lockfile.fs@1100.0.1
  - @pnpm/lockfile.types@1100.0.1
  - @pnpm/lockfile.utils@1100.0.1
  - @pnpm/lockfile.walker@1100.0.1
  - @pnpm/network.auth-header@1100.0.1
  - @pnpm/workspace.project-manifest-reader@1100.0.1

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- f92ac24: Added `pnpm sbom` command for generating Software Bill of Materials in CycloneDX 1.7 and SPDX 2.3 JSON formats [#9088](https://github.com/pnpm/pnpm/issues/9088).
- 6d56db2: The `pnpm audit` command now also audits dependencies from `pnpm-lock.yaml`, including `configDependencies` and `packageManagerDependencies` along with their transitive dependencies.
- 7721d2e: `pnpm audit --fix` now adds the minimum patched versions to `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` [#10263](https://github.com/pnpm/pnpm/issues/10263).

  When `minimumReleaseAge` is configured, security patches suggested by `pnpm audit` may be blocked because the patched versions are too new. Now, `pnpm audit --fix` automatically adds the minimum patched version for each vulnerability (e.g., `axios@0.21.2`) to `minimumReleaseAgeExclude`, so that `pnpm install` can install the security fix without waiting for it to mature.

- 4158906: Support configuring `auditLevel` in the `pnpm-workspace.yaml` file [#10540](https://github.com/pnpm/pnpm/issues/10540).
- 15549a9: Add the ability to fix vulnerabilities by updating packages in the lockfile instead of adding overrides.

### Patch Changes

- 3c36e8d: Fixed `pnpm audit --json` to respect the `--audit-level` setting for both exit code and output filtering [#10540](https://github.com/pnpm/pnpm/issues/10540).
- 121f64a: Fix `pnpm audit --fix` replacing reference overrides (e.g. `$foo`) with concrete versions [#10325](https://github.com/pnpm/pnpm/issues/10325).
- a969839: fixed help text for audit --ignore-registry-errors
- Updated dependencies [e1ea779]
- Updated dependencies [f92ac24]
- Updated dependencies [7730a7f]
- Updated dependencies [996284f]
- Updated dependencies [6d56db2]
- Updated dependencies [7721d2e]
- Updated dependencies [315cae8]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [4c6c26a]
- Updated dependencies [e2e0a32]
- Updated dependencies [c55c614]
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
- Updated dependencies [0b5ccc9]
- Updated dependencies [1cc61e8]
- Updated dependencies [491a84f]
- Updated dependencies [fb8962f]
- Updated dependencies [f0ae1b9]
- Updated dependencies [9fc552d]
- Updated dependencies [312226c]
- Updated dependencies [b1ad9c7]
- Updated dependencies [121f64a]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [075aa99]
- Updated dependencies [fd511e4]
- Updated dependencies [ae43ac7]
- Updated dependencies [ccec8e7]
- Updated dependencies [98a5f1c]
- Updated dependencies [fd511e4]
- Updated dependencies [fa5a5c6]
- Updated dependencies [4158906]
- Updated dependencies [ac944ef]
- Updated dependencies [d458ab3]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [96704a1]
- Updated dependencies [50fbeca]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [6f806be]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [ace7903]
- Updated dependencies [38b8e35]
- Updated dependencies [394d88c]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [ed1a7fe]
- Updated dependencies [15549a9]
- Updated dependencies [b51bb42]
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
  - @pnpm/cli.common-cli-options-help@1001.0.0
  - @pnpm/deps.compliance.sbom@1000.0.0
  - @pnpm/config.reader@1005.0.0
  - @pnpm/installing.commands@1005.0.0
  - @pnpm/deps.compliance.audit@1003.0.0
  - @pnpm/config.writer@1001.0.0
  - @pnpm/deps.compliance.license-scanner@1002.0.0
  - @pnpm/constants@1002.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/lockfile.fs@1002.0.0
  - @pnpm/lockfile.types@1003.0.0
  - @pnpm/lockfile.utils@1004.0.0
  - @pnpm/cli.utils@1002.0.0
  - @pnpm/workspace.project-manifest-reader@1002.0.0
  - @pnpm/network.auth-header@1001.0.0
  - @pnpm/store.path@1001.0.0
  - @pnpm/lockfile.walker@1002.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/cli.meta@1001.0.0
  - @pnpm/cli.command@1001.0.0
