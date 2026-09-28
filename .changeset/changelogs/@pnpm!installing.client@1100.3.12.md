## 1100.3.12

### Patch Changes

- `pnpm deploy` with a shared lockfile now copies workspace dependencies into the deploy directory, even when `packageImportMethod` is set to `hardlink`. Previously, their files were hard-linked to the workspace sources, so editing a source file also changed the deployed copy [pnpm/pnpm#12176](https://github.com/pnpm/pnpm/issues/12176).

- Updated dependencies:
  - @pnpm/engine.runtime.node-resolver@1101.3.4
  - @pnpm/error@1100.2.1
  - @pnpm/fetching.binary-fetcher@1102.0.17
  - @pnpm/fetching.directory-fetcher@1100.0.36
  - @pnpm/fetching.git-fetcher@1102.0.21
  - @pnpm/fetching.tarball-fetcher@1102.1.5
  - @pnpm/hooks.types@1101.0.5
  - @pnpm/network.auth-header@1101.1.15
  - @pnpm/network.fetch@1100.1.19
  - @pnpm/resolving.default-resolver@1101.0.7
  - @pnpm/resolving.npm-resolver@1104.2.2
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/store.index@1100.3.3
