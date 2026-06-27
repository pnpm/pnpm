# @pnpm/engine.runtime.commands

## 1100.1.7

### Patch Changes

- Updated dependencies [05b95ab]
- Updated dependencies [0ec878d]
- Updated dependencies [852d537]
  - @pnpm/network.fetch@1100.1.4
  - @pnpm/engine.runtime.node-resolver@1101.1.9
  - @pnpm/error@1100.0.1
  - @pnpm/cli.utils@1101.0.13
  - @pnpm/config.reader@1101.10.1

## 1100.1.6

### Patch Changes

- Updated dependencies [302a2f7]
- Updated dependencies [0474a9c]
- Updated dependencies [4ca9247]
  - @pnpm/config.reader@1101.10.0
  - @pnpm/engine.runtime.node-resolver@1101.1.8

## 1100.1.5

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [61810aa]
- Updated dependencies [681b593]
- Updated dependencies [a31faa7]
  - @pnpm/config.reader@1101.9.0
  - @pnpm/cli.utils@1101.0.12
  - @pnpm/engine.runtime.node-resolver@1101.1.7
  - @pnpm/network.fetch@1100.1.3

## 1100.1.4

### Patch Changes

- Updated dependencies [bc9ed78]
- Updated dependencies [615c669]
  - @pnpm/config.reader@1101.8.0
  - @pnpm/engine.runtime.node-resolver@1101.1.6
  - @pnpm/network.fetch@1100.1.2
  - @pnpm/cli.utils@1101.0.11

## 1100.1.3

### Patch Changes

- Updated dependencies [822beb5]
- Updated dependencies [3537020]
- Updated dependencies [894ea6a]
- Updated dependencies [6b5d91a]
- Updated dependencies [027196b]
- Updated dependencies [1017c36]
- Updated dependencies [3d50680]
  - @pnpm/config.reader@1101.7.0
  - @pnpm/engine.runtime.node-resolver@1101.1.5
  - @pnpm/cli.utils@1101.0.10
  - @pnpm/network.fetch@1100.1.1

## 1100.1.2

### Patch Changes

- Updated dependencies [60a1eec]
- Updated dependencies [a017bf3]
  - @pnpm/network.fetch@1100.1.0
  - @pnpm/config.reader@1101.6.0
  - @pnpm/engine.runtime.node-resolver@1101.1.4
  - @pnpm/cli.utils@1101.0.9

## 1100.1.1

### Patch Changes

- Updated dependencies [b1fa2d5]
- Updated dependencies [a39a83d]
  - @pnpm/network.fetch@1100.0.8
  - @pnpm/config.reader@1101.5.0
  - @pnpm/engine.runtime.node-resolver@1101.1.3

## 1100.1.0

### Minor Changes

- a662de4: `pnpm runtime set <name> <version>` now saves the runtime to `devEngines.runtime` by default instead of `engines.runtime`. Pass `--save-prod` (or `-P`) to save it to `engines.runtime` instead [#11948](https://github.com/pnpm/pnpm/issues/11948).

### Patch Changes

- Updated dependencies [a23956e]
- Updated dependencies [35d2355]
  - @pnpm/config.reader@1101.4.1
  - @pnpm/engine.runtime.node-resolver@1101.1.2
  - @pnpm/cli.utils@1101.0.8
  - @pnpm/network.fetch@1100.0.7

## 1100.0.17

### Patch Changes

- Updated dependencies [3b62f9d]
- Updated dependencies [212315d]
  - @pnpm/config.reader@1101.4.0
  - @pnpm/cli.utils@1101.0.7
  - @pnpm/engine.runtime.node-resolver@1101.1.1

## 1100.0.16

### Patch Changes

- Updated dependencies [3687b0e]
- Updated dependencies [ced20cb]
- Updated dependencies [d1b340f]
- Updated dependencies [1627943]
  - @pnpm/config.reader@1101.3.3
  - @pnpm/engine.runtime.node-resolver@1101.1.0
  - @pnpm/cli.utils@1101.0.6
  - @pnpm/network.fetch@1100.0.6

## 1100.0.15

### Patch Changes

- Updated dependencies [020ac45]
- Updated dependencies [d3f8408]
- Updated dependencies [247d70b]
- Updated dependencies [a62f959]
- Updated dependencies [ba2c884]
- Updated dependencies [8df408c]
  - @pnpm/config.reader@1101.3.2
  - @pnpm/exec.pnpm-cli-runner@1100.0.1
  - @pnpm/engine.runtime.node-resolver@1101.0.9
  - @pnpm/network.fetch@1100.0.5
  - @pnpm/cli.utils@1101.0.5

## 1100.0.14

### Patch Changes

- Updated dependencies [18a464f]
  - @pnpm/network.fetch@1100.0.4
  - @pnpm/cli.utils@1101.0.4
  - @pnpm/config.reader@1101.3.1
  - @pnpm/engine.runtime.node-resolver@1101.0.8

## 1100.0.13

### Patch Changes

- a575dd2: `pnpm runtime set <name> <version>` no longer fails in the root of a multi-package workspace with the `ADDING_TO_ROOT` error. Installing the workspace root is a valid target for a runtime, so the command now bypasses that safety check.
- Updated dependencies [20e7aff]
- Updated dependencies [b61e268]
- Updated dependencies [e1e29c1]
  - @pnpm/network.fetch@1100.0.3
  - @pnpm/config.reader@1101.3.0
  - @pnpm/engine.runtime.node-resolver@1101.0.7
  - @pnpm/cli.utils@1101.0.3

## 1100.0.12

### Patch Changes

- Updated dependencies [e9e876c]
  - @pnpm/config.reader@1101.2.2
  - @pnpm/engine.runtime.node-resolver@1101.0.6

## 1100.0.11

### Patch Changes

- Updated dependencies [707a879]
  - @pnpm/config.reader@1101.2.1
  - @pnpm/engine.runtime.node-resolver@1101.0.5

## 1100.0.10

### Patch Changes

- Updated dependencies [8fdd9a9]
- Updated dependencies [5f34a8d]
- Updated dependencies [c969392]
- Updated dependencies [817b1b4]
- Updated dependencies [c969392]
- Updated dependencies [2de318b]
  - @pnpm/config.reader@1101.2.0
  - @pnpm/engine.runtime.node-resolver@1101.0.4

## 1100.0.9

### Patch Changes

- Updated dependencies [42a8f29]
  - @pnpm/config.reader@1101.1.4
  - @pnpm/engine.runtime.node-resolver@1101.0.3

## 1100.0.8

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/config.reader@1101.1.3
  - @pnpm/network.fetch@1100.0.2
  - @pnpm/cli.utils@1101.0.2
  - @pnpm/engine.runtime.node-resolver@1101.0.2

## 1100.0.7

### Patch Changes

- @pnpm/cli.utils@1101.0.1

## 1100.0.6

### Patch Changes

- Updated dependencies [0fbcf74]
  - @pnpm/config.reader@1101.1.2
  - @pnpm/engine.runtime.node-resolver@1101.0.1

## 1100.0.5

### Patch Changes

- 9b23098: Updated `pnpm env` help examples to use Node.js 24 and its LTS codename.

## 1100.0.4

### Patch Changes

- Updated dependencies [421317c]
  - @pnpm/engine.runtime.node-resolver@1101.0.0
  - @pnpm/config.reader@1101.1.1

## 1100.0.3

### Patch Changes

- Updated dependencies [7d25bc1]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0
  - @pnpm/engine.runtime.node-resolver@1100.0.3

## 1100.0.2

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/cli.utils@1101.0.0
  - @pnpm/config.reader@1101.0.0
  - @pnpm/engine.runtime.node-resolver@1100.0.2

## 1100.0.1

### Patch Changes

- @pnpm/cli.utils@1100.0.1
- @pnpm/config.reader@1100.0.1
- @pnpm/engine.runtime.node-resolver@1100.0.1
- @pnpm/network.fetch@1100.0.1

## 1000.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.
- 71de2b3: Removed support for the `useNodeVersion` and `executionEnv.nodeVersion` fields. `devEngines.runtime` and `engines.runtime` should be used instead [#10373](https://github.com/pnpm/pnpm/pull/10373).

### Minor Changes

- 9065f49: On systems using the musl C library (e.g. Alpine Linux), `pnpm env use` now automatically downloads the musl variant of Node.js from [unofficial-builds.nodejs.org](https://unofficial-builds.nodejs.org).

  `pnpm env use` now installs Node.js via `pnpm add --global`, so Node.js versions are managed as regular global packages. Running `pnpm store prune` will clean up unused Node.js versions automatically.

  The `pnpm env add` and `pnpm env remove` subcommands have been removed. Use `pnpm env use` to install and activate a Node.js version. `pnpm env list` now only lists remote Node.js versions (the `--remote` flag is no longer required).

### Patch Changes

- 23eb4a6: `parseNodeSpecifier` is moved from `@pnpm/plugin-commands-env` to `@pnpm/engine.runtime.node-resolver` and enhanced to support all Node.js version specifier formats. Previously `parseEnvSpecifier` (in `@pnpm/engine.runtime.node-resolver`) handled the resolver's parsing, while `parseNodeSpecifier` (in `@pnpm/plugin-commands-env`) was a stricter but now-unused validator. They are now unified into a single `parseNodeSpecifier` in `@pnpm/engine.runtime.node-resolver` that supports: exact versions (`22.0.0`), prerelease versions (`22.0.0-rc.4`), semver ranges (`18`, `^18`), LTS codenames (`argon`, `iron`), well-known aliases (`lts`, `latest`), standalone release channels (`nightly`, `rc`, `test`, `v8-canary`, `release`), and channel/version combos (`rc/18`, `nightly/latest`).
- 50fbeca: Added `getNodeBinsForCurrentOS` to `@pnpm/constants` which returns a `Record<string, string>` with paths for `node`, `npm`, and `npx` within the Node.js package. This record is now used as `BinaryResolution.bin` (type widened from `string` to `string | Record<string, string>`) and as `manifest.bin` in the node resolver, so pnpm's bin-linker creates all three shims automatically when installing a Node.js runtime.
- 2efb5d2: Added a new command `pnpm runtime set <runtime name> <runtime version spec> [-g]` for installing runtimes. Deprecated `pnpm env use` in favor of the new command.
- Updated dependencies [7730a7f]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [3c72b6b]
- Updated dependencies [9f5c0e3]
- Updated dependencies [76718b3]
- Updated dependencies [cc1b8e3]
- Updated dependencies [90bd3c3]
- Updated dependencies [1cc61e8]
- Updated dependencies [606f53e]
- Updated dependencies [c7203b9]
- Updated dependencies [bb17724]
- Updated dependencies [da2429d]
- Updated dependencies [1cc61e8]
- Updated dependencies [491a84f]
- Updated dependencies [f0ae1b9]
- Updated dependencies [0dfa8b8]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [075aa99]
- Updated dependencies [23eb4a6]
- Updated dependencies [ae43ac7]
- Updated dependencies [ccec8e7]
- Updated dependencies [4158906]
- Updated dependencies [ac944ef]
- Updated dependencies [9065f49]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [96704a1]
- Updated dependencies [50fbeca]
- Updated dependencies [bb8baa7]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [6c480a4]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [499ef22]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [ed1a7fe]
- Updated dependencies [cc7c0d2]
- Updated dependencies [5bf7768]
- Updated dependencies [ae43ac7]
- Updated dependencies [a5fdbf9]
- Updated dependencies [9d3f00b]
- Updated dependencies [6b3d87a]
- Updated dependencies [9587dac]
- Updated dependencies [09a999a]
- Updated dependencies [559f903]
- Updated dependencies [3574905]
  - @pnpm/config.reader@1005.0.0
  - @pnpm/cli.utils@1002.0.0
  - @pnpm/exec.pnpm-cli-runner@1001.0.0
  - @pnpm/engine.runtime.node-resolver@1002.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/network.fetch@1001.0.0
