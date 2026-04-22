# @pnpm/building.after-install

## 1101.0.0

### Patch Changes

- Updated dependencies [421317c]
  - @pnpm/store.cafs@1100.1.0
  - @pnpm/worker@1100.1.0
  - @pnpm/store.controller-types@1100.0.3
  - @pnpm/store.connection-manager@1100.0.4
  - @pnpm/exec.lifecycle@1100.0.4
  - @pnpm/lockfile.utils@1100.0.3
  - @pnpm/installing.context@1100.0.3
  - @pnpm/deps.graph-hasher@1100.1.1
  - @pnpm/config.reader@1101.1.1

## 1100.0.3

### Patch Changes

- 72c1e05: Fix: different platform variants of the same runtime (e.g. `node@runtime:25.9.0` glibc vs. musl) no longer share a single global-virtual-store entry. The virtual store path now incorporates the selected variant's integrity, so installs with different `--os`/`--cpu`/`--libc` end up in separate directories and `pnpm add --libc=musl node@runtime:<v>` reliably fetches the musl binary even when the glibc variant is already cached.
- Updated dependencies [7d25bc1]
- Updated dependencies [72c1e05]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0
  - @pnpm/deps.graph-hasher@1100.1.0
  - @pnpm/store.connection-manager@1100.0.3
  - @pnpm/exec.lifecycle@1100.0.3
  - @pnpm/installing.context@1100.0.2
  - @pnpm/lockfile.types@1100.0.2
  - @pnpm/lockfile.utils@1100.0.2
  - @pnpm/store.controller-types@1100.0.2
  - @pnpm/store.cafs@1100.0.2
  - @pnpm/lockfile.walker@1100.0.2
  - @pnpm/worker@1100.0.2

## 1100.0.2

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/config.reader@1101.0.0
  - @pnpm/store.connection-manager@1100.0.2
  - @pnpm/bins.linker@1100.0.2
  - @pnpm/exec.lifecycle@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/bins.linker@1100.0.1
  - @pnpm/building.pkg-requires-build@1100.0.1
  - @pnpm/building.policy@1100.0.1
  - @pnpm/config.normalize-registries@1100.0.1
  - @pnpm/config.reader@1100.0.1
  - @pnpm/core-loggers@1100.0.1
  - @pnpm/deps.graph-hasher@1100.0.1
  - @pnpm/deps.path@1100.0.1
  - @pnpm/exec.lifecycle@1100.0.1
  - @pnpm/installing.context@1100.0.1
  - @pnpm/installing.modules-yaml@1100.0.1
  - @pnpm/lockfile.types@1100.0.1
  - @pnpm/lockfile.utils@1100.0.1
  - @pnpm/lockfile.walker@1100.0.1
  - @pnpm/pkg-manifest.reader@1100.0.1
  - @pnpm/store.cafs@1100.0.1
  - @pnpm/store.controller-types@1100.0.1
  - @pnpm/worker@1100.0.1
  - @pnpm/store.connection-manager@1100.0.1

## 1000.0.0

### Major Changes

- 2fccb03: Initial release
- 7354e6b: Initial release.

### Patch Changes

- 996284f: Allow `pnpm approve-builds` to receive positional arguments for approving or denying packages without the interactive prompt. Prefix a package name with `!` to deny it. Only mentioned packages are affected; the rest are left untouched.

  During install, packages with ignored builds that are not yet listed in `allowBuilds` are automatically added with a placeholder value. This makes them visible in `pnpm-workspace.yaml` so users can manually change them to `true` or `false` without running `pnpm approve-builds`.

- Updated dependencies [7730a7f]
- Updated dependencies [5f73b0f]
- Updated dependencies [449dacf]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [e2e0a32]
- Updated dependencies [c55c614]
- Updated dependencies [3c72b6b]
- Updated dependencies [5d130c3]
- Updated dependencies [9f5c0e3]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [90bd3c3]
- Updated dependencies [7cec347]
- Updated dependencies [3cfffaa]
- Updated dependencies [1cc61e8]
- Updated dependencies [606f53e]
- Updated dependencies [c7203b9]
- Updated dependencies [bb17724]
- Updated dependencies [2fccb03]
- Updated dependencies [82f4610]
- Updated dependencies [05fb1ae]
- Updated dependencies [cd743ef]
- Updated dependencies [da2429d]
- Updated dependencies [1cc61e8]
- Updated dependencies [3bf5e21]
- Updated dependencies [491a84f]
- Updated dependencies [6656baa]
- Updated dependencies [62f760e]
- Updated dependencies [f0ae1b9]
- Updated dependencies [6e9cad3]
- Updated dependencies [2ea6463]
- Updated dependencies [50fbeca]
- Updated dependencies [cb228c9]
- Updated dependencies [caabba4]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [9eddabb]
- Updated dependencies [075aa99]
- Updated dependencies [c4045fc]
- Updated dependencies [ae43ac7]
- Updated dependencies [ccec8e7]
- Updated dependencies [ba065f6]
- Updated dependencies [3bf5e21]
- Updated dependencies [4158906]
- Updated dependencies [ac944ef]
- Updated dependencies [878a773]
- Updated dependencies [f8e6774]
- Updated dependencies [ee9fe58]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
- Updated dependencies [56a59df]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [780af09]
- Updated dependencies [96704a1]
- Updated dependencies [50fbeca]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [f40177f]
- Updated dependencies [71de2b3]
- Updated dependencies [4893853]
- Updated dependencies [10bc391]
- Updated dependencies [38b8e35]
- Updated dependencies [394d88c]
- Updated dependencies [b7f0f21]
- Updated dependencies [1e6de25]
- Updated dependencies [831f574]
- Updated dependencies [366cabe]
- Updated dependencies [2df8b71]
- Updated dependencies [ed1a7fe]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [5bf7768]
- Updated dependencies [3cfffaa]
- Updated dependencies [ae43ac7]
- Updated dependencies [a5fdbf9]
- Updated dependencies [7354e6b]
- Updated dependencies [9d3f00b]
- Updated dependencies [98a0410]
- Updated dependencies [efb48dc]
- Updated dependencies [9587dac]
- Updated dependencies [09a999a]
- Updated dependencies [559f903]
- Updated dependencies [3574905]
- Updated dependencies [f871365]
  - @pnpm/config.reader@1005.0.0
  - @pnpm/deps.path@1002.0.0
  - @pnpm/deps.graph-hasher@1003.0.0
  - @pnpm/bins.linker@1001.0.0
  - @pnpm/store.controller-types@1005.0.0
  - @pnpm/store.cafs@1001.0.0
  - @pnpm/worker@1001.0.0
  - @pnpm/constants@1002.0.0
  - @pnpm/installing.context@1002.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/lockfile.types@1003.0.0
  - @pnpm/lockfile.utils@1004.0.0
  - @pnpm/installing.modules-yaml@1001.0.0
  - @pnpm/building.pkg-requires-build@1000.0.0
  - @pnpm/building.policy@1000.0.0
  - @pnpm/pkg-manifest.reader@1001.0.0
  - @pnpm/store.connection-manager@1003.0.0
  - @pnpm/config.normalize-registries@1001.0.0
  - @pnpm/core-loggers@1002.0.0
  - @pnpm/deps.graph-sequencer@1001.0.0
  - @pnpm/lockfile.walker@1002.0.0
  - @pnpm/exec.lifecycle@1002.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/store.index@1000.0.0
