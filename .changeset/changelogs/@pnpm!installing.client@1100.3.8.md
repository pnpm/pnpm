## 1100.3.8

### Patch Changes

- Authenticate Node.js runtime downloads from `nodeDownloadMirrors` with URL-scoped npm registry credentials, including bearer tokens, basic auth, and `tokenHelper` [pnpm/pnpm#14334](https://github.com/pnpm/pnpm/issues/14334).

- Updated dependencies:
  - @pnpm/engine.runtime.node-resolver@1101.3.0
  - @pnpm/error@1100.1.4
  - @pnpm/fetching.binary-fetcher@1102.0.13
  - @pnpm/fetching.directory-fetcher@1100.0.32
  - @pnpm/fetching.git-fetcher@1102.0.17
  - @pnpm/fetching.tarball-fetcher@1102.1.1
  - @pnpm/hooks.types@1101.0.2
  - @pnpm/network.auth-header@1101.1.13
  - @pnpm/network.fetch@1100.1.15
  - @pnpm/resolving.default-resolver@1101.0.3
  - @pnpm/resolving.npm-resolver@1104.1.1
  - @pnpm/store.index@1100.3.1
