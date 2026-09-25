## 1100.1.18

### Patch Changes

- `pnpm install` now fails at once when a registry or tarball server presents a TLS certificate that fails verification, such as a self-signed or expired one. The error names the certificate problem. Such requests were retried for more than a minute [#9134](https://github.com/pnpm/pnpm/issues/9134).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/error@1100.2.0
  - @pnpm/types@1102.1.1
