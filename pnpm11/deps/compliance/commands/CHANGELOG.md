# @pnpm/deps.compliance.commands

## 1101.5.0

### Minor Changes

- 6c35a43: Added `--exclude-peers` to `pnpm sbom`. With `auto-install-peers` (the default), peer dependencies resolve into the lockfile and are otherwise indistinguishable from the package's own dependencies. The flag drops peer dependencies (and any transitive subtree reachable only through them) from the SBOM. CycloneDX 1.7 has no scope or relationship that expresses "consumer-provided peer", so omission is the only spec-clean handling. The flag name matches `pnpm list --exclude-peers`; note the SBOM flag prunes a peer's exclusive subtree, which is stricter than `pnpm list` (which only hides leaf peers).

### Patch Changes

- 25a829e: `pnpm audit --fix` now writes a single combined `minimumReleaseAgeExclude` entry per package (e.g. `axios@0.18.1 || 0.21.1`) instead of one entry per version, matching the format documented for the setting. Existing per-version entries in `pnpm-workspace.yaml` are merged into the combined form rather than left as duplicates. Installs that auto-collect immature versions into `minimumReleaseAgeExclude` now report the same combined entries, so the "Added N entries" message matches what is written to the manifest [#12534](https://github.com/pnpm/pnpm/issues/12534).
- 17e7f2c: `pnpm sbom` now emits a CycloneDX `issue-tracker` external reference for components (and the root) whose `package.json` declares a `bugs` URL. Email-only `bugs` entries are skipped, since the reference requires a URL.
- Updated dependencies [25a829e]
- Updated dependencies [bae694f]
- Updated dependencies [d3f68e2]
- Updated dependencies [6545793]
- Updated dependencies [fbdc0eb]
- Updated dependencies [0ec878d]
- Updated dependencies [6c35a43]
- Updated dependencies [17e7f2c]
- Updated dependencies [a84d2a1]
- Updated dependencies [852d537]
  - @pnpm/installing.commands@1100.10.1
  - @pnpm/config.version-policy@1100.1.6
  - @pnpm/lockfile.utils@1100.1.0
  - @pnpm/deps.compliance.audit@1101.0.18
  - @pnpm/workspace.project-manifest-reader@1100.0.14
  - @pnpm/deps.compliance.sbom@1100.3.0
  - @pnpm/error@1100.0.1
  - @pnpm/config.writer@1100.0.14
  - @pnpm/lockfile.types@1100.0.12
  - @pnpm/deps.compliance.license-scanner@1100.0.21
  - @pnpm/lockfile.fs@1100.1.7
  - @pnpm/deps.security.signatures@1101.2.3
  - @pnpm/cli.utils@1101.0.13
  - @pnpm/config.reader@1101.10.1
  - @pnpm/network.auth-header@1101.1.3
  - @pnpm/store.path@1100.0.2
  - @pnpm/lockfile.walker@1100.0.12

## 1101.4.0

### Minor Changes

- 1495cb0: Added per-package SBOM generation with `--out` and `--split` flags. Use `--out out/%s.cdx.json` to write one SBOM per workspace package to individual files, or `--split` for NDJSON output to stdout. When `--filter` selects a single package, the SBOM root component now uses that package's metadata. Workspace inter-dependencies (`workspace:` protocol) and their transitive dependencies are included. Author, repository, and license fall back to the root manifest when the package doesn't define them.

### Patch Changes

- Updated dependencies [302a2f7]
- Updated dependencies [c112b61]
- Updated dependencies [61969fb]
- Updated dependencies [9d79ba1]
- Updated dependencies [0474a9c]
- Updated dependencies [223d060]
- Updated dependencies [0474a9c]
- Updated dependencies [6d35338]
- Updated dependencies [dcededc]
- Updated dependencies [1495cb0]
- Updated dependencies [eba03e0]
  - @pnpm/config.reader@1101.10.0
  - @pnpm/installing.commands@1100.10.0
  - @pnpm/lockfile.fs@1100.1.6
  - @pnpm/deps.compliance.sbom@1100.2.0
  - @pnpm/deps.compliance.audit@1101.0.17
  - @pnpm/deps.compliance.license-scanner@1100.0.20

## 1101.3.5

### Patch Changes

- 8dcd9a0: Fix garbled summary line after submitting `pnpm update -i` and `pnpm audit --fix -i`. The interactive checkbox prompt previously printed every selected choice's full table row (label, current/target versions, workspace, URL) joined by commas, producing a wall of text after pressing Enter. The summary now lists only the selected package names (or vulnerability keys) by setting an explicit `short` per choice; the in-progress selection UI is unchanged.
- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [8dcd9a0]
- Updated dependencies [86e70d2]
- Updated dependencies [61810aa]
- Updated dependencies [f20ad8f]
- Updated dependencies [ab0b7d1]
- Updated dependencies [74a2dc9]
- Updated dependencies [681b593]
- Updated dependencies [d50d691]
- Updated dependencies [a31faa7]
  - @pnpm/installing.commands@1100.9.0
  - @pnpm/config.reader@1101.9.0
  - @pnpm/lockfile.utils@1100.0.13
  - @pnpm/network.auth-header@1101.1.2
  - @pnpm/types@1101.3.2
  - @pnpm/lockfile.fs@1100.1.5
  - @pnpm/cli.utils@1101.0.12
  - @pnpm/deps.compliance.audit@1101.0.16
  - @pnpm/deps.compliance.license-scanner@1100.0.19
  - @pnpm/deps.compliance.sbom@1100.1.9
  - @pnpm/deps.security.signatures@1101.2.2
  - @pnpm/object.key-sorting@1100.0.1
  - @pnpm/workspace.project-manifest-reader@1100.0.13
  - @pnpm/cli.meta@1100.0.8
  - @pnpm/config.pick-registry-for-package@1100.0.9
  - @pnpm/config.writer@1100.0.13
  - @pnpm/lockfile.types@1100.0.11
  - @pnpm/lockfile.walker@1100.0.11

## 1101.3.4

### Patch Changes

- Updated dependencies [bc9ed78]
- Updated dependencies [d976edf]
- Updated dependencies [615c669]
  - @pnpm/config.reader@1101.8.0
  - @pnpm/installing.commands@1100.8.0
  - @pnpm/cli.utils@1101.0.11
  - @pnpm/deps.compliance.license-scanner@1100.0.18
  - @pnpm/deps.compliance.audit@1101.0.15
  - @pnpm/deps.security.signatures@1101.2.1
  - @pnpm/workspace.project-manifest-reader@1100.0.12
  - @pnpm/deps.compliance.sbom@1100.1.8

## 1101.3.3

### Patch Changes

- Updated dependencies [822beb5]
- Updated dependencies [3537020]
- Updated dependencies [894ea6a]
- Updated dependencies [6b5d91a]
- Updated dependencies [027196b]
- Updated dependencies [5f2bb9f]
- Updated dependencies [1017c36]
- Updated dependencies [e4d2fe0]
- Updated dependencies [bf1b731]
  - @pnpm/config.reader@1101.7.0
  - @pnpm/deps.security.signatures@1101.2.0
  - @pnpm/installing.commands@1100.7.3
  - @pnpm/cli.common-cli-options-help@1100.0.2
  - @pnpm/types@1101.3.1
  - @pnpm/cli.meta@1100.0.7
  - @pnpm/cli.utils@1101.0.10
  - @pnpm/config.pick-registry-for-package@1100.0.8
  - @pnpm/config.writer@1100.0.12
  - @pnpm/deps.compliance.audit@1101.0.14
  - @pnpm/deps.compliance.license-scanner@1100.0.17
  - @pnpm/deps.compliance.sbom@1100.1.7
  - @pnpm/lockfile.fs@1100.1.4
  - @pnpm/lockfile.types@1100.0.10
  - @pnpm/lockfile.utils@1100.0.12
  - @pnpm/lockfile.walker@1100.0.10
  - @pnpm/network.auth-header@1101.1.1
  - @pnpm/workspace.project-manifest-reader@1100.0.11

## 1101.3.2

### Patch Changes

- Updated dependencies [5192edf]
- Updated dependencies [a017bf3]
  - @pnpm/network.auth-header@1101.1.0
  - @pnpm/config.reader@1101.6.0
  - @pnpm/types@1101.3.0
  - @pnpm/installing.commands@1100.7.2
  - @pnpm/deps.compliance.audit@1101.0.13
  - @pnpm/deps.security.signatures@1101.1.6
  - @pnpm/cli.meta@1100.0.6
  - @pnpm/cli.utils@1101.0.9
  - @pnpm/config.pick-registry-for-package@1100.0.7
  - @pnpm/config.writer@1100.0.11
  - @pnpm/deps.compliance.license-scanner@1100.0.16
  - @pnpm/deps.compliance.sbom@1100.1.6
  - @pnpm/lockfile.fs@1100.1.3
  - @pnpm/lockfile.types@1100.0.9
  - @pnpm/lockfile.utils@1100.0.11
  - @pnpm/lockfile.walker@1100.0.9
  - @pnpm/workspace.project-manifest-reader@1100.0.10

## 1101.3.1

### Patch Changes

- Updated dependencies [719cc21]
  - @pnpm/deps.compliance.audit@1101.0.12
  - @pnpm/installing.commands@1100.7.1

## 1101.3.0

### Minor Changes

- 2cadfb5: Replaced `enquirer` with `@inquirer/prompts` for all interactive prompts. Fixes the `update -i` scrolling overflow bug where long choice lists were clipped in the terminal [#6643](https://github.com/pnpm/pnpm/issues/6643).

  **User-facing changes:**

  - `pnpm update -i` / `pnpm update -i --latest`: Scrolling now works correctly when many packages are available; the new library uses visual-line-aware pagination via `usePagination`
  - `pnpm audit --fix -i`: Same scrolling fix for vulnerability selection
  - `pnpm approve-builds`: Interactive build approval prompts updated
  - `pnpm patch`: Version selection and "apply to all" prompts updated
  - `pnpm patch-remove`: Patch removal selection updated
  - `pnpm publish`: Branch confirmation prompt updated
  - `pnpm login`: Credential prompts updated
  - `pnpm run` / `pnpm exec` (with `verifyDepsBeforeRun=prompt`): Confirmation prompt updated

  Vim-style `j`/`k` keys still work for up/down navigation in all interactive prompts.

  **Internal:** The `OtpEnquirer` and `LoginEnquirer` DI interfaces changed from `{ prompt }` to `{ input }` / `{ input, password }` respectively. Plugins or custom builds that inject their own enquirer mock will need to update.

### Patch Changes

- Updated dependencies [a39a83d]
- Updated dependencies [2cadfb5]
  - @pnpm/config.reader@1101.5.0
  - @pnpm/installing.commands@1100.7.0
  - @pnpm/deps.compliance.audit@1101.0.11
  - @pnpm/deps.security.signatures@1101.1.5

## 1101.2.8

### Patch Changes

- Updated dependencies [a23956e]
- Updated dependencies [aa6149d]
- Updated dependencies [a456dc7]
- Updated dependencies [572842a]
- Updated dependencies [e55f4b5]
- Updated dependencies [35d2355]
  - @pnpm/config.reader@1101.4.1
  - @pnpm/network.auth-header@1101.0.0
  - @pnpm/installing.commands@1100.6.0
  - @pnpm/workspace.project-manifest-reader@1100.0.9
  - @pnpm/lockfile.utils@1100.0.10
  - @pnpm/types@1101.2.0
  - @pnpm/cli.utils@1101.0.8
  - @pnpm/deps.compliance.audit@1101.0.10
  - @pnpm/deps.compliance.license-scanner@1100.0.15
  - @pnpm/deps.compliance.sbom@1100.1.5
  - @pnpm/lockfile.fs@1100.1.2
  - @pnpm/cli.meta@1100.0.5
  - @pnpm/config.pick-registry-for-package@1100.0.6
  - @pnpm/config.writer@1100.0.10
  - @pnpm/lockfile.types@1100.0.8
  - @pnpm/lockfile.walker@1100.0.8
  - @pnpm/deps.security.signatures@1101.1.4

## 1101.2.7

### Patch Changes

- Updated dependencies [d7da112]
- Updated dependencies [3b62f9d]
- Updated dependencies [212315d]
  - @pnpm/workspace.project-manifest-reader@1100.0.8
  - @pnpm/config.reader@1101.4.0
  - @pnpm/installing.commands@1100.5.0
  - @pnpm/cli.utils@1101.0.7
  - @pnpm/deps.compliance.license-scanner@1100.0.14
  - @pnpm/deps.compliance.sbom@1100.1.4

## 1101.2.6

### Patch Changes

- Updated dependencies [881a865]
  - @pnpm/installing.commands@1100.4.2

## 1101.2.5

### Patch Changes

- Updated dependencies [097983f]
  - @pnpm/config.pick-registry-for-package@1100.0.5
  - @pnpm/installing.commands@1100.4.1

## 1101.2.4

### Patch Changes

- Updated dependencies [3687b0e]
- Updated dependencies [ced20cb]
- Updated dependencies [a620557]
- Updated dependencies [9cb48bb]
- Updated dependencies [d1b340f]
- Updated dependencies [b206a15]
- Updated dependencies [64afc92]
  - @pnpm/config.reader@1101.3.3
  - @pnpm/installing.commands@1100.4.0
  - @pnpm/lockfile.fs@1100.1.1
  - @pnpm/types@1101.1.1
  - @pnpm/deps.compliance.audit@1101.0.9
  - @pnpm/deps.compliance.license-scanner@1100.0.13
  - @pnpm/deps.compliance.sbom@1100.1.3
  - @pnpm/lockfile.types@1100.0.7
  - @pnpm/lockfile.utils@1100.0.9
  - @pnpm/cli.utils@1101.0.6
  - @pnpm/workspace.project-manifest-reader@1100.0.7
  - @pnpm/cli.meta@1100.0.4
  - @pnpm/config.pick-registry-for-package@1100.0.4
  - @pnpm/config.writer@1100.0.9
  - @pnpm/lockfile.walker@1100.0.7
  - @pnpm/network.auth-header@1100.0.3
  - @pnpm/deps.security.signatures@1101.1.3

## 1101.2.3

### Patch Changes

- Updated dependencies [4195766]
- Updated dependencies [020ac45]
- Updated dependencies [d3f8408]
- Updated dependencies [6e93f35]
- Updated dependencies [a62f959]
- Updated dependencies [ba2c884]
- Updated dependencies [2a9bd89]
- Updated dependencies [8df408c]
  - @pnpm/installing.commands@1100.3.0
  - @pnpm/config.reader@1101.3.2
  - @pnpm/lockfile.fs@1100.1.0
  - @pnpm/deps.compliance.sbom@1100.1.2
  - @pnpm/lockfile.types@1100.0.6
  - @pnpm/lockfile.utils@1100.0.8
  - @pnpm/deps.compliance.audit@1101.0.8
  - @pnpm/deps.compliance.license-scanner@1100.0.12
  - @pnpm/lockfile.walker@1100.0.6
  - @pnpm/cli.utils@1101.0.5
  - @pnpm/deps.security.signatures@1101.1.2
  - @pnpm/workspace.project-manifest-reader@1100.0.6
  - @pnpm/config.writer@1100.0.8

## 1101.2.2

### Patch Changes

- Updated dependencies [180aee9]
  - @pnpm/installing.commands@1100.2.2
  - @pnpm/lockfile.fs@1100.0.8
  - @pnpm/cli.utils@1101.0.4
  - @pnpm/config.reader@1101.3.1
  - @pnpm/workspace.project-manifest-reader@1100.0.5
  - @pnpm/deps.compliance.audit@1101.0.7
  - @pnpm/deps.security.signatures@1101.1.1
  - @pnpm/deps.compliance.license-scanner@1100.0.11
  - @pnpm/deps.compliance.sbom@1100.1.1

## 1101.2.1

### Patch Changes

- @pnpm/installing.commands@1100.2.1

## 1101.2.0

### Minor Changes

- 6ac06cb: Added `pnpm audit signatures` to verify ECDSA registry signatures for installed packages against keys from `/-/npm/v1/keys` [#7909](https://github.com/pnpm/pnpm/issues/7909). Scoped registries are respected, and registries without signing keys are skipped.

### Patch Changes

- Updated dependencies [6ac06cb]
- Updated dependencies [b61e268]
- Updated dependencies [87b4bac]
- Updated dependencies [e1e29c1]
  - @pnpm/deps.security.signatures@1101.1.0
  - @pnpm/config.reader@1101.3.0
  - @pnpm/types@1101.1.0
  - @pnpm/deps.compliance.sbom@1100.1.0
  - @pnpm/installing.commands@1100.2.0
  - @pnpm/deps.compliance.audit@1101.0.6
  - @pnpm/cli.meta@1100.0.3
  - @pnpm/cli.utils@1101.0.3
  - @pnpm/config.pick-registry-for-package@1100.0.3
  - @pnpm/config.writer@1100.0.7
  - @pnpm/deps.compliance.license-scanner@1100.0.10
  - @pnpm/lockfile.fs@1100.0.7
  - @pnpm/lockfile.types@1100.0.5
  - @pnpm/lockfile.utils@1100.0.7
  - @pnpm/lockfile.walker@1100.0.5
  - @pnpm/network.auth-header@1100.0.2
  - @pnpm/workspace.project-manifest-reader@1100.0.4

## 1101.1.11

### Patch Changes

- Updated dependencies [e9e876c]
- Updated dependencies [15e9e35]
  - @pnpm/config.reader@1101.2.2
  - @pnpm/installing.commands@1100.1.12
  - @pnpm/deps.compliance.license-scanner@1100.0.9
  - @pnpm/deps.compliance.sbom@1100.0.9

## 1101.1.10

### Patch Changes

- Updated dependencies [cfa271b]
  - @pnpm/lockfile.utils@1100.0.6
  - @pnpm/deps.compliance.audit@1101.0.5
  - @pnpm/deps.compliance.license-scanner@1100.0.8
  - @pnpm/deps.compliance.sbom@1100.0.8
  - @pnpm/lockfile.fs@1100.0.6
  - @pnpm/installing.commands@1100.1.11

## 1101.1.9

### Patch Changes

- Updated dependencies [27425d7]
- Updated dependencies [707a879]
  - @pnpm/lockfile.fs@1100.0.5
  - @pnpm/lockfile.types@1100.0.4
  - @pnpm/lockfile.utils@1100.0.5
  - @pnpm/config.reader@1101.2.1
  - @pnpm/installing.commands@1100.1.10
  - @pnpm/deps.compliance.audit@1101.0.4
  - @pnpm/deps.compliance.license-scanner@1100.0.7
  - @pnpm/deps.compliance.sbom@1100.0.7
  - @pnpm/lockfile.walker@1100.0.4
  - @pnpm/config.writer@1100.0.6

## 1101.1.8

### Patch Changes

- Updated dependencies [8fdd9a9]
- Updated dependencies [5f34a8d]
- Updated dependencies [c969392]
- Updated dependencies [817b1b4]
- Updated dependencies [c969392]
- Updated dependencies [2de318b]
  - @pnpm/config.reader@1101.2.0
  - @pnpm/installing.commands@1100.1.9
  - @pnpm/config.writer@1100.0.5

## 1101.1.7

### Patch Changes

- Updated dependencies [f6bc1db]
  - @pnpm/installing.commands@1100.1.8

## 1101.1.6

### Patch Changes

- Updated dependencies [42a8f29]
  - @pnpm/config.reader@1101.1.4
  - @pnpm/installing.commands@1100.1.7

## 1101.1.5

### Patch Changes

- Updated dependencies [184ce26]
- Updated dependencies [6b891a5]
  - @pnpm/workspace.project-manifest-reader@1100.0.3
  - @pnpm/deps.compliance.license-scanner@1100.0.6
  - @pnpm/cli.common-cli-options-help@1100.0.1
  - @pnpm/deps.compliance.audit@1101.0.3
  - @pnpm/installing.commands@1100.1.6
  - @pnpm/config.reader@1101.1.3
  - @pnpm/config.writer@1100.0.4
  - @pnpm/cli.command@1100.0.1
  - @pnpm/store.path@1100.0.1
  - @pnpm/cli.utils@1101.0.2
  - @pnpm/cli.meta@1100.0.2
  - @pnpm/lockfile.utils@1100.0.4
  - @pnpm/deps.compliance.sbom@1100.0.6
  - @pnpm/lockfile.types@1100.0.3
  - @pnpm/lockfile.fs@1100.0.4
  - @pnpm/lockfile.walker@1100.0.3

## 1101.1.4

### Patch Changes

- @pnpm/cli.utils@1101.0.1
- @pnpm/deps.compliance.license-scanner@1100.0.5
- @pnpm/installing.commands@1100.1.5

## 1101.1.3

### Patch Changes

- 5e11362: Sort the keys of the overrides object returned by `pnpm audit --fix` so that the log output order matches the order written to `pnpm-workspace.yaml`.
- Updated dependencies [f9afe81]
- Updated dependencies [0fbcf74]
  - @pnpm/deps.compliance.sbom@1100.0.5
  - @pnpm/config.reader@1101.1.2
  - @pnpm/installing.commands@1100.1.4
  - @pnpm/config.writer@1100.0.3

## 1101.1.2

### Patch Changes

- @pnpm/installing.commands@1100.1.3

## 1101.1.1

### Patch Changes

- @pnpm/deps.compliance.license-scanner@1100.0.4
- @pnpm/deps.compliance.sbom@1100.0.4
- @pnpm/installing.commands@1100.1.2
- @pnpm/lockfile.utils@1100.0.3
- @pnpm/deps.compliance.audit@1101.0.2
- @pnpm/lockfile.fs@1100.0.3
- @pnpm/config.reader@1101.1.1

## 1101.1.0

### Minor Changes

- 390ee62: `pnpm audit --fix` now respects the `auditLevel` setting and supports a new interactive mode via `--interactive`/`-i`. Previously, `pnpm audit --fix` would fix all vulnerabilities regardless of the configured `auditLevel`, while `pnpm audit` (without `--fix`) correctly filtered by severity. Now both commands consistently filter advisories by the `auditLevel` setting, and you can use `pnpm audit --fix -i` to review and select which vulnerabilities to fix interactively.

  Overrides emitted by `pnpm audit --fix` now use a caret range (`^X.Y.Z`) instead of an open-ended `>=X.Y.Z`, so applying a security fix can no longer silently promote a dependency across a major version boundary.

### Patch Changes

- 61952c2: `pnpm sbom` now detects licenses declared via the deprecated `licenses` array in `package.json` (e.g. `busboy`, `streamsearch`, `limiter`) and falls back to scanning on-disk `LICENSE` files — mirroring the resolution logic of `pnpm licenses`. Previously these packages were reported as `NOASSERTION`. Shared license resolution (manifest parsing + LICENSE-file fallback) lives in the new `@pnpm/deps.compliance.license-resolver` package. When a manifest sets both `license` and `licenses`, the modern `license` field now takes precedence for both commands (previously `pnpm licenses` preferred `licenses`) [#11248](https://github.com/pnpm/pnpm/issues/11248).
- Updated dependencies [7d25bc1]
- Updated dependencies [9e0833c]
- Updated dependencies [61952c2]
  - @pnpm/config.reader@1101.1.0
  - @pnpm/deps.compliance.license-resolver@1100.0.0
  - @pnpm/deps.compliance.sbom@1100.0.3
  - @pnpm/deps.compliance.license-scanner@1100.0.3
  - @pnpm/installing.commands@1100.1.1
  - @pnpm/lockfile.types@1100.0.2
  - @pnpm/lockfile.utils@1100.0.2
  - @pnpm/deps.compliance.audit@1101.0.1
  - @pnpm/lockfile.fs@1100.0.2
  - @pnpm/lockfile.walker@1100.0.2
  - @pnpm/config.writer@1100.0.2

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
