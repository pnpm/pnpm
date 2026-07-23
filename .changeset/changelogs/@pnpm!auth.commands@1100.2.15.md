## 1100.2.15

### Patch Changes

- Republished every package: the tarballs published by the v11.13.1 through v11.16.0 releases were missing most of their compiled files due to a packing bug [#13164](https://github.com/pnpm/pnpm/issues/13164).

- When the authentication URL cannot be rendered as a QR code (for example when it exceeds the maximum QR data capacity), web-based login now displays the URL alone with a warning instead of aborting authentication [pnpm/pnpm#12721](https://github.com/pnpm/pnpm/issues/12721).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.19
  - @pnpm/config.reader@1101.14.0
  - @pnpm/error@1100.1.0
  - @pnpm/network.fetch@1100.1.8
  - @pnpm/network.web-auth@1101.3.0
  - @pnpm/registry-access.client@1100.1.10
