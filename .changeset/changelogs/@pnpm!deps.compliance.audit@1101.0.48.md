## 1101.0.48

### Patch Changes

- `pnpm audit` now reports advisories whose `id` is a string. Such advisories were dropped, and `pnpm audit` reported no known vulnerabilities [#18184](https://github.com/pnpm/pnpm/issues/18184).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.10
  - @pnpm/lockfile.detect-dep-types@1100.0.31
  - @pnpm/lockfile.fs@1100.2.19
  - @pnpm/lockfile.peer-edges@1100.0.6
  - @pnpm/lockfile.types@1100.1.8
  - @pnpm/lockfile.utils@1102.1.12
  - @pnpm/lockfile.walker@1100.0.31
  - @pnpm/network.fetch@1100.1.26
  - @pnpm/types@1102.1.4
