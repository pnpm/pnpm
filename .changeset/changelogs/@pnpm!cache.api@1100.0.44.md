## 1100.0.44

### Patch Changes

- `pnpm cache list-registries` now prints the registry URL, matching `pnpm cache view`. It printed `https%3A+registry.npmjs.org` before and prints `https://registry.npmjs.org/` now [#15046](https://github.com/pnpm/pnpm/issues/15046).

- Updated dependencies:
  - @pnpm/config.reader@1102.2.1
  - @pnpm/resolving.npm-resolver@1104.2.0
  - @pnpm/store.cafs@1100.3.3
