## 1100.0.31

### Patch Changes

- `pnpm install` now detects a `supportedArchitectures` change and re-evaluates previously skipped platform-specific optional dependencies, instead of reporting the project as up to date and leaving the packages for the old architecture set in place.

- Updated dependencies:
  - @pnpm/config.reader@1101.12.3
