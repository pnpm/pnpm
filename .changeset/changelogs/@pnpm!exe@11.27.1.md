## 11.27.1

### Patch Changes

- `pn`, `pnpx`, and `pnx` now run the pnpm installed alongside them. They used to look pnpm up on `PATH`. That failed when the directory holding them was not on `PATH`, and it silently handed the call to an unrelated pnpm when one came first there [#14803](https://github.com/pnpm/pnpm/issues/14803).

- Updated dependencies:
  - @pnpm/linux-arm64@11.27.1
  - @pnpm/linux-x64@11.27.1
  - @pnpm/linuxstatic-arm64@11.27.1
  - @pnpm/linuxstatic-x64@11.27.1
  - @pnpm/macos-arm64@11.27.1
  - @pnpm/win-arm64@11.27.1
  - @pnpm/win-x64@11.27.1
