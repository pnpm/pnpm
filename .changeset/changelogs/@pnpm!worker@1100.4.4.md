## 1100.4.4

### Patch Changes

- A dependency that ships a `binding.gyp` and sets `gypfile: false` no longer gets the `node-gyp rebuild` install script pnpm synthesizes for it. Such a dependency needs no `allowBuilds` entry and is no longer listed under "Ignored build scripts".

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.18
  - @pnpm/crypto.integrity@1100.0.7
  - @pnpm/error@1100.2.0
  - @pnpm/fs.graceful-fs@1100.2.3
  - @pnpm/fs.hard-link-dir@1100.0.7
  - @pnpm/fs.symlink-dependency@1100.0.21
  - @pnpm/store.cafs@1100.3.4
  - @pnpm/store.create-cafs-store@1100.0.31
  - @pnpm/store.index@1100.3.2
