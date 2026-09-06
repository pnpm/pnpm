## 1100.1.15

### Patch Changes

- Authenticate Node.js runtime downloads from `nodeDownloadMirrors` with URL-scoped npm registry credentials, including bearer tokens, basic auth, and `tokenHelper` [pnpm/pnpm#14334](https://github.com/pnpm/pnpm/issues/14334).

- Network and archive retry logs hide credentials and signed query parameters in request URLs.

- Updated dependencies:
  - @pnpm/error@1100.1.4
