## 1101.0.32

### Patch Changes

- `pnpm audit` no longer reports a patched version that was never published or is deprecated. The inferred patched range (e.g. `>=4.17.24` from `<=4.17.23`) is now checked against the registry packument, and the report is corrected to the lowest non-deprecated published version that satisfies it (e.g. `>=4.18.1` when `4.17.24` does not exist and `4.18.0` is deprecated). When no published version satisfies the range, the report shows `Patched versions: None`. This also prevents `pnpm audit --fix` from adding overrides or `minimumReleaseAgeExclude` entries for patches that do not exist [#13824](https://github.com/pnpm/pnpm/issues/13824).

  `pnpm audit --fix` and `pnpm audit --fix update` no longer add a `minimumReleaseAgeExclude` entry when the registry packument shows that the minimum patched version was never published. Previously such entries were written for versions that do not exist, which would have let a later publish of that version bypass the `minimumReleaseAge` gate [#11563](https://github.com/pnpm/pnpm/issues/11563).

  The `--json` output of `pnpm audit` now returns `patched_versions: null` for advisories whose inferred patch is not available (never published, skipped, yanked, or deprecated), making it easier for tooling to distinguish "no fix available" from "fix available at version X".

- Updated dependencies:
  - @pnpm/deps.path@1101.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/lockfile.detect-dep-types@1100.0.20
  - @pnpm/lockfile.fs@1100.2.3
  - @pnpm/lockfile.types@1100.0.20
  - @pnpm/lockfile.utils@1102.0.0
  - @pnpm/lockfile.walker@1100.0.20
  - @pnpm/network.fetch@1100.1.13
  - @pnpm/types@1102.0.0
