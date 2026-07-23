## 1101.3.0

### Minor Changes

- The token poll for web-based authentication no longer reads the body of non-OK or still-pending (HTTP 202) responses, and caps the token response body it does read at 64 KiB, so a malicious or compromised registry cannot exhaust memory through the poll [pnpm/pnpm#12721](https://github.com/pnpm/pnpm/issues/12721).

- When the authentication URL cannot be rendered as a QR code (for example when it exceeds the maximum QR data capacity), web-based login now displays the URL alone with a warning instead of aborting authentication [pnpm/pnpm#12721](https://github.com/pnpm/pnpm/issues/12721).

### Patch Changes

- Republished every package: the tarballs published by the v11.13.1 through v11.16.0 releases were missing most of their compiled files due to a packing bug [#13164](https://github.com/pnpm/pnpm/issues/13164).

- Updated dependencies:
  - @pnpm/error@1100.1.0
