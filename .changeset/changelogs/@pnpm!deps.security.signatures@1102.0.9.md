## 1102.0.9

### Patch Changes

- `pnpm audit signatures` now verifies signatures against the integrity recorded in the lockfile. Packages without a recorded integrity cannot pass signature verification.

- Updated dependencies:
  - @pnpm/error@1100.2.2
  - @pnpm/network.fetch@1100.1.22
