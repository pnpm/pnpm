# @pnpm/store.pkg-finder

## 1100.0.7

### Patch Changes

- Updated dependencies [0c67cb5]
  - @pnpm/store.index@1100.1.0
  - @pnpm/fetching.directory-fetcher@1100.0.7

## 1100.0.6

### Patch Changes

- 27425d7: Pin the integrity of git-hosted tarballs (codeload.github.com, gitlab.com, bitbucket.org) in the lockfile so that subsequent installs detect a tampered or substituted tarball and refuse to install it. Previously the lockfile only stored the tarball URL for git dependencies, so a compromised git host or a man-in-the-middle could serve arbitrary code on later installs without lockfile changes.

  A new `gitHosted: true` field is recorded on git-hosted tarball resolutions in the lockfile, letting every reader/writer route them by a single typed check instead of pattern-matching the tarball URL in each call site. Lockfiles written by older pnpm versions are enriched on load (URL fallback) so the field can be relied on uniformly across the codebase.

- Updated dependencies [27425d7]
  - @pnpm/resolving.resolver-base@1100.1.2
  - @pnpm/fetching.directory-fetcher@1100.0.6
  - @pnpm/store.cafs@1100.1.2

## 1100.0.5

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/fetching.directory-fetcher@1100.0.5
  - @pnpm/resolving.resolver-base@1100.1.1
  - @pnpm/deps.path@1100.0.2
  - @pnpm/store.cafs@1100.1.1

## 1100.0.4

### Patch Changes

- Updated dependencies [421317c]
  - @pnpm/store.cafs@1100.1.0
  - @pnpm/fetching.directory-fetcher@1100.0.4

## 1100.0.3

### Patch Changes

- bcc88a1: Fixed `pnpm sbom` and `pnpm licenses` failing to resolve license information for git-sourced dependencies (`git+https://`, `git+ssh://`, `github:` shorthand). These commands now correctly read the package manifest from the content-addressable store for `type: 'git'` resolutions [#11260](https://github.com/pnpm/pnpm/issues/11260).
- Updated dependencies [e03e8f4]
- Updated dependencies [72c1e05]
  - @pnpm/fetching.directory-fetcher@1100.0.3
  - @pnpm/resolving.resolver-base@1100.1.0
  - @pnpm/store.cafs@1100.0.2

## 1100.0.2

### Patch Changes

- @pnpm/fetching.directory-fetcher@1100.0.2

## 1100.0.1

### Patch Changes

- @pnpm/deps.path@1100.0.1
- @pnpm/fetching.directory-fetcher@1100.0.1
- @pnpm/resolving.resolver-base@1100.0.1
- @pnpm/store.cafs@1100.0.1

## 1000.0.0

### Major Changes

- f92ac24: Initial release.

### Minor Changes

- b7f0f21: Use SQLite for storing package index in the content-addressable store. Instead of individual `.mpk` files under `$STORE/index/`, package metadata is now stored in a single SQLite database at `$STORE/index.db`. This reduces filesystem syscall overhead, improves space efficiency for small metadata entries, and enables concurrent access via SQLite's WAL mode. Packages missing from the new index are re-fetched on demand [#10826](https://github.com/pnpm/pnpm/issues/10826).

### Patch Changes

- Updated dependencies [5f73b0f]
- Updated dependencies [facdd71]
- Updated dependencies [e2e0a32]
- Updated dependencies [9b0a460]
- Updated dependencies [3bf5e21]
- Updated dependencies [491a84f]
- Updated dependencies [6656baa]
- Updated dependencies [2ea6463]
- Updated dependencies [50fbeca]
- Updated dependencies [caabba4]
- Updated dependencies [878a773]
- Updated dependencies [f8e6774]
- Updated dependencies [7d2fd48]
- Updated dependencies [56a59df]
- Updated dependencies [50fbeca]
- Updated dependencies [10bc391]
- Updated dependencies [38b8e35]
- Updated dependencies [b7f0f21]
- Updated dependencies [1e6de25]
- Updated dependencies [9d3f00b]
  - @pnpm/deps.path@1002.0.0
  - @pnpm/resolving.resolver-base@1006.0.0
  - @pnpm/store.cafs@1001.0.0
  - @pnpm/fetching.directory-fetcher@1001.0.0
  - @pnpm/store.index@1000.0.0
