## 11.28.8

### Patch Changes

- The standalone `@pnpm/exe` builds now embed Node.js 26.12.1. The previously embedded Node.js 26.0.0 still trusted the certificate authority intermediates that Node.js distrusted in October 2026, so a standalone pnpm accepted TLS certificates issued under them.

- Updated dependencies:
  - @pnpm/linux-arm64@11.28.8
  - @pnpm/linux-x64@11.28.8
  - @pnpm/linuxstatic-x64@11.28.8
  - @pnpm/macos-arm64@11.28.8
  - @pnpm/win-arm64@11.28.8
  - @pnpm/win-x64@11.28.8
