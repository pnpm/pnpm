## 1101.1.1

### Patch Changes

- Authenticate Node.js runtime downloads from `nodeDownloadMirrors` with URL-scoped npm registry credentials, including bearer tokens, basic auth, and `tokenHelper` [pnpm/pnpm#14334](https://github.com/pnpm/pnpm/issues/14334).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.26
  - @pnpm/config.reader@1102.1.1
  - @pnpm/engine.runtime.node-resolver@1101.3.0
  - @pnpm/error@1100.1.4
  - @pnpm/network.auth-header@1101.1.13
  - @pnpm/network.fetch@1100.1.15
