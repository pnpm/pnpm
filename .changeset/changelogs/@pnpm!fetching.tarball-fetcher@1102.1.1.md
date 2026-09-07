## 1102.1.1

### Patch Changes

- Authenticate Node.js runtime downloads from `nodeDownloadMirrors` with URL-scoped npm registry credentials, including bearer tokens, basic auth, and `tokenHelper` [pnpm/pnpm#14334](https://github.com/pnpm/pnpm/issues/14334).

- Network and archive retry logs hide credentials and signed query parameters in request URLs.

- Updated dependencies:
  - @pnpm/error@1100.1.4
  - @pnpm/exec.prepare-package@1100.0.35
  - @pnpm/store.index@1100.3.1
