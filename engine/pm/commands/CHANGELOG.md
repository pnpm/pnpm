# @pnpm/engine.pm.commands

## 1100.0.1

### Patch Changes

- b989a4a: Fixed `pnpm store prune` removing packages used by the globally installed pnpm, breaking it.
- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/bins.linker@1100.0.1
  - @pnpm/building.policy@1100.0.1
  - @pnpm/cli.meta@1100.0.1
  - @pnpm/cli.utils@1100.0.1
  - @pnpm/config.reader@1100.0.1
  - @pnpm/deps.graph-hasher@1100.0.1
  - @pnpm/global.commands@1100.0.1
  - @pnpm/global.packages@1100.0.1
  - @pnpm/installing.client@1100.0.1
  - @pnpm/installing.deps-restorer@1100.0.1
  - @pnpm/installing.env-installer@1100.0.1
  - @pnpm/lockfile.types@1100.0.1
  - @pnpm/resolving.npm-resolver@1100.0.1
  - @pnpm/store.controller@1100.0.1
  - @pnpm/workspace.project-manifest-reader@1100.0.1
  - @pnpm/store.connection-manager@1100.0.1

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- a8f016c: Store config dependency and package manager integrity info in `pnpm-lock.yaml` instead of inlining it in `pnpm-workspace.yaml`. The workspace manifest now contains only clean version specifiers for `configDependencies`, while the resolved versions, integrity hashes, and tarball URLs are recorded in the lockfile as a separate YAML document. The env lockfile section also stores `packageManagerDependencies` resolved during version switching and self-update. Projects using the old inline-hash format are automatically migrated on install.
- f0ae1b9: Store globally installed binaries in a `bin` subdirectory of `PNPM_HOME` instead of directly in `PNPM_HOME`. This prevents internal directories like `global/` and `store/` from polluting shell autocompletion when `PNPM_HOME` is on PATH [#10986](https://github.com/pnpm/pnpm/issues/10986).

  After upgrading, run `pnpm setup` to update your shell configuration.

### Patch Changes

- 46f1016: `pnpm self-update` should always install the non-executable pnpm package (pnpm in the registry) and never the `@pnpm/exe` package, when installing v11 or newer. We currently cannot ship `@pnpm/exe` as `pkg` doesn't work with ESM [#10190](https://github.com/pnpm/pnpm/pull/10190).
- 253858d: Fixed `pnpm self-update` breaking when running `@pnpm/exe`. The platform binary (e.g., `@pnpm/macos-arm64`) was not found in pnpm's symlinked `node_modules` layout because it was looked up at the top level instead of as a sibling of `@pnpm/exe` in the virtual store.
- 1ab0f7b: Fixed version switching via `packageManager` field failing when pnpm is installed as a standalone executable in environments without a system Node.js [#10687](https://github.com/pnpm/pnpm/issues/10687).
- Updated dependencies [ac4c9f4]
- Updated dependencies [7730a7f]
- Updated dependencies [5f73b0f]
- Updated dependencies [449dacf]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [394d88c]
- Updated dependencies [e2e0a32]
- Updated dependencies [3c72b6b]
- Updated dependencies [9f5c0e3]
- Updated dependencies [a297ebc]
- Updated dependencies [76718b3]
- Updated dependencies [821b36a]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [5a0ed1d]
- Updated dependencies [90bd3c3]
- Updated dependencies [1cc61e8]
- Updated dependencies [606f53e]
- Updated dependencies [831f574]
- Updated dependencies [0e9c559]
- Updated dependencies [c7203b9]
- Updated dependencies [bb17724]
- Updated dependencies [2fccb03]
- Updated dependencies [82f4610]
- Updated dependencies [05fb1ae]
- Updated dependencies [cd743ef]
- Updated dependencies [da2429d]
- Updated dependencies [1cc61e8]
- Updated dependencies [19f36cf]
- Updated dependencies [491a84f]
- Updated dependencies [62f760e]
- Updated dependencies [f0ae1b9]
- Updated dependencies [9fc552d]
- Updated dependencies [394d88c]
- Updated dependencies [6e9cad3]
- Updated dependencies [61cad0c]
- Updated dependencies [312226c]
- Updated dependencies [cb228c9]
- Updated dependencies [19f36cf]
- Updated dependencies [d8be970]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [9eddabb]
- Updated dependencies [075aa99]
- Updated dependencies [c4045fc]
- Updated dependencies [fd511e4]
- Updated dependencies [ae43ac7]
- Updated dependencies [ccec8e7]
- Updated dependencies [98a5f1c]
- Updated dependencies [143ca78]
- Updated dependencies [ba065f6]
- Updated dependencies [4158906]
- Updated dependencies [6f361aa]
- Updated dependencies [ac944ef]
- Updated dependencies [0625e20]
- Updated dependencies [938ea1f]
- Updated dependencies [2cb0657]
- Updated dependencies [bb8baa7]
- Updated dependencies [7d2fd48]
- Updated dependencies [9eddabb]
- Updated dependencies [cc7c0d2]
- Updated dependencies [144ce0e]
- Updated dependencies [efb48dc]
- Updated dependencies [56a59df]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [96704a1]
- Updated dependencies [50fbeca]
- Updated dependencies [4a36b9a]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [f40177f]
- Updated dependencies [615bd24]
- Updated dependencies [05158d2]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [ba70035]
- Updated dependencies [3585d9a]
- Updated dependencies [38b8e35]
- Updated dependencies [b7f0f21]
- Updated dependencies [1e6de25]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [2f98ec8]
- Updated dependencies [ed1a7fe]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [5bf7768]
- Updated dependencies [ae43ac7]
- Updated dependencies [09bb8db]
- Updated dependencies [a5fdbf9]
- Updated dependencies [7354e6b]
- Updated dependencies [9d3f00b]
- Updated dependencies [6557dc0]
- Updated dependencies [efb48dc]
- Updated dependencies [9587dac]
- Updated dependencies [09a999a]
- Updated dependencies [559f903]
- Updated dependencies [3574905]
- Updated dependencies [4362c06]
  - @pnpm/installing.deps-restorer@1007.0.0
  - @pnpm/config.reader@1005.0.0
  - @pnpm/deps.graph-hasher@1003.0.0
  - @pnpm/bins.linker@1001.0.0
  - @pnpm/resolving.npm-resolver@1005.0.0
  - @pnpm/store.controller@1005.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/installing.env-installer@1001.0.0
  - @pnpm/lockfile.types@1003.0.0
  - @pnpm/cli.utils@1002.0.0
  - @pnpm/building.policy@1000.0.0
  - @pnpm/workspace.project-manifest-reader@1002.0.0
  - @pnpm/store.connection-manager@1003.0.0
  - @pnpm/installing.client@1002.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/cli.meta@1001.0.0
  - @pnpm/global.packages@1000.0.0
  - @pnpm/global.commands@1000.0.0
