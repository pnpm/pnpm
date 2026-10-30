## 1100.3.19

### Patch Changes

- `pnpm install` now retries a Node.js runtime zip download when the connection drops partway through. A download that still fails names its URL. Before, a single dropped connection ended the install with `terminated` [#17411](https://github.com/pnpm/pnpm/issues/17411).

- Updated dependencies:
  - @pnpm/engine.runtime.node-resolver@1101.3.11
  - @pnpm/fetching.binary-fetcher@1102.0.23
  - @pnpm/fetching.directory-fetcher@1100.0.42
  - @pnpm/fetching.git-fetcher@1102.0.28
  - @pnpm/fetching.tarball-fetcher@1102.1.12
  - @pnpm/resolving.default-resolver@1101.0.14
