## 1100.1.24

### Patch Changes

- A registry response with too many content encodings can no longer crash pnpm. pnpm now uses undici 7.30.1, which fixes [GHSA-xjj6-f33q-929c](https://github.com/advisories/GHSA-xjj6-f33q-929c).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.4
  - @pnpm/types@1102.1.3
