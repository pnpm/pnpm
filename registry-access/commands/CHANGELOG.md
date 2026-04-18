# @pnpm/registry-access.commands

## 1100.2.0

### Minor Changes

- 75942bf: Implemented native `star`, `unstar`, `stars`, and `whoami` commands.

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/cli.utils@1101.0.0
  - @pnpm/config.reader@1101.0.0

## 1100.1.0

### Minor Changes

- b738043: Add native `pnpm ping` command to test registry connectivity.
  Provides a simple way to verify connectivity to the configured registry without requiring external tools.
- f2083f4: Implemented native `search` command and its aliases (`s`, `se`, `find`).

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/cli.utils@1100.0.1
  - @pnpm/config.pick-registry-for-package@1100.0.1
  - @pnpm/config.reader@1100.0.1
  - @pnpm/network.auth-header@1100.0.1
  - @pnpm/network.fetch@1100.0.1
  - @pnpm/resolving.registry.types@1100.0.1

## 1000.1.0

### Minor Changes

- 853be66: Added native `pnpm dist-tag` command with `ls`, `add`, and `rm` subcommands [#11128](https://github.com/pnpm/pnpm/issues/11128).
- 2c90d40: Added native `pnpm deprecate` and `pnpm undeprecate` commands that interact with the npm registry directly instead of delegating to the npm CLI [#11120](https://github.com/pnpm/pnpm/pull/11120).

### Patch Changes

- Updated dependencies [7730a7f]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [3c72b6b]
- Updated dependencies [9f5c0e3]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [90bd3c3]
- Updated dependencies [1cc61e8]
- Updated dependencies [606f53e]
- Updated dependencies [c7203b9]
- Updated dependencies [bb17724]
- Updated dependencies [da2429d]
- Updated dependencies [1cc61e8]
- Updated dependencies [491a84f]
- Updated dependencies [fb8962f]
- Updated dependencies [f0ae1b9]
- Updated dependencies [b1ad9c7]
- Updated dependencies [0dfa8b8]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [075aa99]
- Updated dependencies [ae43ac7]
- Updated dependencies [ccec8e7]
- Updated dependencies [4158906]
- Updated dependencies [ac944ef]
- Updated dependencies [d3d6938]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [96704a1]
- Updated dependencies [bb8baa7]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [6c480a4]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [ed1a7fe]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [5bf7768]
- Updated dependencies [ae43ac7]
- Updated dependencies [a5fdbf9]
- Updated dependencies [9d3f00b]
- Updated dependencies [efb48dc]
- Updated dependencies [6b3d87a]
- Updated dependencies [9587dac]
- Updated dependencies [09a999a]
- Updated dependencies [559f903]
- Updated dependencies [3574905]
  - @pnpm/config.reader@1005.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/cli.utils@1002.0.0
  - @pnpm/config.pick-registry-for-package@1001.0.0
  - @pnpm/network.auth-header@1001.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/network.fetch@1001.0.0
  - @pnpm/resolving.registry.types@1000.1.0
