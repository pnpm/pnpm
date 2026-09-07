## 1100.1.8

### Patch Changes

- `pnpm outdated` and `pnpm update` now follow local actions and reusable workflows referenced with GitHub's self-repository syntax (`uses: $/.github/actions/setup`) when looking for outdated GitHub Actions, the same way they follow `./` references.

- Updated dependencies:
  - @pnpm/error@1100.1.4
  - @pnpm/resolving.git-resolver@1100.1.20
