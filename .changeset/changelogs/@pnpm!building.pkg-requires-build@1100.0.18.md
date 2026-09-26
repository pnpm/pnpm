## 1100.0.18

### Patch Changes

- A dependency that ships a `binding.gyp` and sets `gypfile: false` no longer gets the `node-gyp rebuild` install script pnpm synthesizes for it. Such a dependency needs no `allowBuilds` entry and is no longer listed under "Ignored build scripts".

- Updated dependencies:
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/types@1102.1.1
