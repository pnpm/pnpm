## 1100.3.4

### Patch Changes

- `pnpm add` and `pnpm install` now support installing bzip2 compressed tarballs [https://github.com/pnpm/pnpm/issues/6761](https://github.com/pnpm/pnpm/issues/6761).

- A dependency that ships a `binding.gyp` and sets `gypfile: false` no longer gets the `node-gyp rebuild` install script pnpm synthesizes for it. Such a dependency needs no `allowBuilds` entry and is no longer listed under "Ignored build scripts".

- Updated dependencies:
  - @pnpm/error@1100.2.0
  - @pnpm/fetching.fetcher-base@1100.2.11
  - @pnpm/fs.graceful-fs@1100.2.3
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/types@1102.1.1
