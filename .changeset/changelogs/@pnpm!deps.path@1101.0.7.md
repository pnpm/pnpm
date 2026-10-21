## 1101.0.7

### Patch Changes

- An optional peer that pnpm adds to a project now keeps the version the lockfile recorded for that project only while the dependency graph still holds it. Otherwise it follows its provider to a version the graph holds. Before, pnpm kept a higher version that only the lockfile held, which left two copies of the peer [#16443](https://github.com/pnpm/pnpm/issues/16443).

- Updated dependencies:
  - @pnpm/types@1102.1.2
