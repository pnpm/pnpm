## 1101.3.3

### Patch Changes

- Node.js runtime resolution now supports Windows ARM64. Node.js 20 and newer resolve native `win-arm64` builds, and older versions fall back to `win-x64` under emulation [#7123](https://github.com/pnpm/pnpm/issues/7123).

- Updated dependencies:
  - @pnpm/config.reader@1102.3.0
  - @pnpm/crypto.shasums-file@1100.2.6
  - @pnpm/error@1100.2.0
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/types@1102.1.1
