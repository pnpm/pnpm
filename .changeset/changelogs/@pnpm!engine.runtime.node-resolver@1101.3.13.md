## 1101.3.13

### Patch Changes

- `pnpm dedupe` now keeps a locked runtime version, from `devEngines.runtime` or from a dependency's `engines.runtime`, when the declared range still admits it. `pnpm dedupe --check` no longer fails each time a newer Node.js release in the range comes out [#17984](https://github.com/pnpm/pnpm/issues/17984).

- Updated dependencies:
  - @pnpm/config.reader@1102.3.9
  - @pnpm/crypto.shasums-file@1100.2.9
  - @pnpm/resolving.resolver-base@1101.3.5
