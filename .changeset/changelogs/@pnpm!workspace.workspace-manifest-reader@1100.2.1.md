## 1100.2.1

### Patch Changes

- A falsy non-array `packages` field in `pnpm-workspace.yaml`, such as `packages: false`, is now rejected with an error instead of being treated as omitted.

- Updated dependencies:
  - @pnpm/error@1100.2.1
