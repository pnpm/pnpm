## 1100.3.1

### Patch Changes

- Fixed global virtual store hashes for dependency cycles. Every package that transitively depends on an allowed build now includes the engine in its store path, independent of traversal order [pnpm/pnpm#14341](https://github.com/pnpm/pnpm/issues/14341).

- Updated dependencies:
  - @pnpm/lockfile.types@1100.1.1
  - @pnpm/lockfile.utils@1102.1.1
