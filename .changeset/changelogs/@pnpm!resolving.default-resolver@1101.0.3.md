## 1101.0.3

### Patch Changes

- Authenticate Node.js runtime downloads from `nodeDownloadMirrors` with URL-scoped npm registry credentials, including bearer tokens, basic auth, and `tokenHelper` [pnpm/pnpm#14334](https://github.com/pnpm/pnpm/issues/14334).

- Updated dependencies:
  - @pnpm/engine.runtime.bun-resolver@1102.0.19
  - @pnpm/engine.runtime.deno-resolver@1102.0.19
  - @pnpm/engine.runtime.node-resolver@1101.3.0
  - @pnpm/error@1100.1.4
  - @pnpm/hooks.types@1101.0.2
  - @pnpm/network.auth-header@1101.1.13
  - @pnpm/resolving.git-resolver@1100.1.20
  - @pnpm/resolving.local-resolver@1101.2.0
  - @pnpm/resolving.npm-resolver@1104.1.1
