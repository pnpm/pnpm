## 1100.2.5

### Patch Changes

- Fixed an issue where package overrides were written into the metadata cache, causing removed overrides to keep applying on subsequent installs [pnpm/pnpm#13918](https://github.com/pnpm/pnpm/issues/13918).

- Updated dependencies:
  - @pnpm/config.parse-overrides@1100.1.4
  - @pnpm/error@1100.1.3
  - @pnpm/types@1102.0.0
