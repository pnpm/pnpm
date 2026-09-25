## 1102.1.4

### Patch Changes

- `pnpm install` now fails at once when a registry or tarball server presents a TLS certificate that fails verification, such as a self-signed or expired one. The error names the certificate problem. Such requests were retried for more than a minute [#9134](https://github.com/pnpm/pnpm/issues/9134).

- `pnpm import` and fresh resolutions now record `integrity` for git-hosted tarballs, such as `codeload.github.com` URLs, even when the tarball is already in the store [#13338](https://github.com/pnpm/pnpm/issues/13338).

- When the registry stops sending data for longer than `fetchTimeout`, pnpm now reports that the metadata or tarball request timed out. Previously the error did not mention the timeout [#3646](https://github.com/pnpm/pnpm/issues/3646).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/error@1100.2.0
  - @pnpm/exec.prepare-package@1100.0.38
  - @pnpm/fetching.fetcher-base@1100.2.11
  - @pnpm/fs.graceful-fs@1100.2.3
  - @pnpm/fs.packlist@1100.0.5
  - @pnpm/network.fetch@1100.1.18
  - @pnpm/store.index@1100.3.2
  - @pnpm/types@1102.1.1
