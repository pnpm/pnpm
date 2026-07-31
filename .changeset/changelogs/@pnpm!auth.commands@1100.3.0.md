## 1100.3.0

### Minor Changes

- `pnpm login` no longer requires an interactive terminal when the registry supports web-based login: without a TTY it prints the authentication URL (skipping the QR code and the "Press ENTER to open the URL in your browser" prompt) and polls the registry until the browser approval completes. Only the classic username/password login still fails with `ERR_PNPM_LOGIN_NON_INTERACTIVE` in a non-interactive terminal.

### Patch Changes

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.21
  - @pnpm/config.reader@1101.15.1
  - @pnpm/network.fetch@1100.1.10
  - @pnpm/network.web-auth@1101.4.0
  - @pnpm/registry-access.client@1100.1.12
