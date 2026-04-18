# @pnpm/engine.runtime.commands

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
