## 1101.4.0

### Minor Changes

- `pnpm login` no longer requires an interactive terminal when the registry supports web-based login: without a TTY it prints the authentication URL (skipping the QR code and the "Press ENTER to open the URL in your browser" prompt) and polls the registry until the browser approval completes. Only the classic username/password login still fails with `ERR_PNPM_LOGIN_NON_INTERACTIVE` in a non-interactive terminal.
