# @pnpm/registry-access.commands

## 1100.3.6

### Patch Changes

- Updated dependencies [05b95ab]
- Updated dependencies [852d537]
  - @pnpm/network.fetch@1100.1.4
  - @pnpm/error@1100.0.1
  - @pnpm/registry-access.client@1100.1.5
  - @pnpm/cli.utils@1101.0.13
  - @pnpm/config.reader@1101.10.1
  - @pnpm/network.auth-header@1101.1.3
  - @pnpm/network.web-auth@1101.1.2

## 1100.3.5

### Patch Changes

- Updated dependencies [302a2f7]
- Updated dependencies [0474a9c]
  - @pnpm/config.reader@1101.10.0

## 1100.3.4

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
  - @pnpm/network.auth-header@1101.1.2
  - @pnpm/types@1101.3.2
  - @pnpm/cli.utils@1101.0.12
  - @pnpm/network.fetch@1100.1.3
  - @pnpm/network.web-auth@1101.1.1
  - @pnpm/config.pick-registry-for-package@1100.0.9
  - @pnpm/resolving.registry.types@1100.1.3
  - @pnpm/registry-access.client@1100.1.4

## 1100.3.3

### Patch Changes

- Updated dependencies [bc9ed78]
- Updated dependencies [615c669]
  - @pnpm/config.reader@1101.8.0
  - @pnpm/network.fetch@1100.1.2
  - @pnpm/cli.utils@1101.0.11
  - @pnpm/registry-access.client@1100.1.3

## 1100.3.2

### Patch Changes

- Updated dependencies [822beb5]
- Updated dependencies [3537020]
- Updated dependencies [894ea6a]
- Updated dependencies [6b5d91a]
- Updated dependencies [027196b]
- Updated dependencies [1017c36]
- Updated dependencies [bf1b731]
  - @pnpm/config.reader@1101.7.0
  - @pnpm/types@1101.3.1
  - @pnpm/cli.utils@1101.0.10
  - @pnpm/config.pick-registry-for-package@1100.0.8
  - @pnpm/network.auth-header@1101.1.1
  - @pnpm/network.fetch@1100.1.1
  - @pnpm/resolving.registry.types@1100.1.2
  - @pnpm/registry-access.client@1100.1.2

## 1100.3.1

### Patch Changes

- Updated dependencies [60a1eec]
- Updated dependencies [5192edf]
- Updated dependencies [a017bf3]
  - @pnpm/network.fetch@1100.1.0
  - @pnpm/network.auth-header@1101.1.0
  - @pnpm/config.reader@1101.6.0
  - @pnpm/types@1101.3.0
  - @pnpm/registry-access.client@1100.1.1
  - @pnpm/cli.utils@1101.0.9
  - @pnpm/config.pick-registry-for-package@1100.0.7
  - @pnpm/resolving.registry.types@1100.1.1

## 1100.3.0

### Minor Changes

- 2cadfb5: Replaced `enquirer` with `@inquirer/prompts` for all interactive prompts. Fixes the `update -i` scrolling overflow bug where long choice lists were clipped in the terminal [#6643](https://github.com/pnpm/pnpm/issues/6643).

  **User-facing changes:**

  - `pnpm update -i` / `pnpm update -i --latest`: Scrolling now works correctly when many packages are available; the new library uses visual-line-aware pagination via `usePagination`
  - `pnpm audit --fix -i`: Same scrolling fix for vulnerability selection
  - `pnpm approve-builds`: Interactive build approval prompts updated
  - `pnpm patch`: Version selection and "apply to all" prompts updated
  - `pnpm patch-remove`: Patch removal selection updated
  - `pnpm publish`: Branch confirmation prompt updated
  - `pnpm login`: Credential prompts updated
  - `pnpm run` / `pnpm exec` (with `verifyDepsBeforeRun=prompt`): Confirmation prompt updated

  Vim-style `j`/`k` keys still work for up/down navigation in all interactive prompts.

  **Internal:** The `OtpEnquirer` and `LoginEnquirer` DI interfaces changed from `{ prompt }` to `{ input }` / `{ input, password }` respectively. Plugins or custom builds that inject their own enquirer mock will need to update.

### Patch Changes

- b1fa2d5: Fix `pnpm dist-tag add` and `pnpm dist-tag rm` against npmjs.org failing without `--otp` with `[ERR_PNPM_UNAUTHORIZED] You must be logged in to set dist-tag … "You must provide a one-time pass. Upgrade your client to npm@latest in order to use 2FA."`. pnpm now sends `npm-auth-type: web` on dist-tag writes and surfaces the resulting OTP challenge through the existing browser-based 2FA flow (the same `withOtpHandling` helper used by `pnpm publish`), so the browser opens, the user authenticates, and the dist-tag is set on retry. `--otp=<code>` continues to work via the classic flow.
- Updated dependencies [b1fa2d5]
- Updated dependencies [a39a83d]
- Updated dependencies [2cadfb5]
- Updated dependencies [1e9ab29]
  - @pnpm/registry-access.client@1100.1.0
  - @pnpm/network.fetch@1100.0.8
  - @pnpm/config.reader@1101.5.0
  - @pnpm/network.web-auth@1101.1.0
  - @pnpm/resolving.registry.types@1100.1.0

## 1100.2.16

### Patch Changes

- ae21758: Refactor the dist-tag-add and login (classic adduser) handlers to delegate their PUTs to a new shared package `@pnpm/registry-access.client`. Downstream tests in this monorepo now use these helpers (via `@pnpm/testing.registry-mock`) instead of `addDistTag` / `addUser` from `@pnpm/registry-mock`, which relied on the unmaintained `anonymous-npm-registry-client`.
- Updated dependencies [a23956e]
- Updated dependencies [35d2355]
  - @pnpm/config.reader@1101.4.1
  - @pnpm/network.auth-header@1101.0.0
  - @pnpm/types@1101.2.0
  - @pnpm/cli.utils@1101.0.8
  - @pnpm/config.pick-registry-for-package@1100.0.6
  - @pnpm/network.fetch@1100.0.7
  - @pnpm/resolving.registry.types@1100.0.5
  - @pnpm/registry-access.client@1100.0.1

## 1100.2.15

### Patch Changes

- Updated dependencies [3b62f9d]
- Updated dependencies [212315d]
  - @pnpm/config.reader@1101.4.0
  - @pnpm/cli.utils@1101.0.7

## 1100.2.14

### Patch Changes

- Updated dependencies [097983f]
  - @pnpm/config.pick-registry-for-package@1100.0.5

## 1100.2.13

### Patch Changes

- d1b340f: Fixed `pnpm login` and `pnpm logout` ignoring `registries.default` from `pnpm-workspace.yaml` [#10099](https://github.com/pnpm/pnpm/issues/10099).
- Updated dependencies [3687b0e]
- Updated dependencies [ced20cb]
- Updated dependencies [d1b340f]
- Updated dependencies [64afc92]
  - @pnpm/config.reader@1101.3.3
  - @pnpm/types@1101.1.1
  - @pnpm/cli.utils@1101.0.6
  - @pnpm/config.pick-registry-for-package@1100.0.4
  - @pnpm/network.auth-header@1100.0.3
  - @pnpm/network.fetch@1100.0.6
  - @pnpm/resolving.registry.types@1100.0.4

## 1100.2.12

### Patch Changes

- Updated dependencies [020ac45]
- Updated dependencies [d3f8408]
- Updated dependencies [a62f959]
- Updated dependencies [ba2c884]
- Updated dependencies [8df408c]
  - @pnpm/config.reader@1101.3.2
  - @pnpm/network.fetch@1100.0.5
  - @pnpm/cli.utils@1101.0.5

## 1100.2.11

### Patch Changes

- Updated dependencies [18a464f]
  - @pnpm/network.fetch@1100.0.4
  - @pnpm/cli.utils@1101.0.4
  - @pnpm/config.reader@1101.3.1

## 1100.2.10

### Patch Changes

- 601317e: Added `pnpm owner` command to manage package owners on the registry.
- Updated dependencies [20e7aff]
- Updated dependencies [b61e268]
- Updated dependencies [e1e29c1]
  - @pnpm/network.fetch@1100.0.3
  - @pnpm/config.reader@1101.3.0
  - @pnpm/types@1101.1.0
  - @pnpm/cli.utils@1101.0.3
  - @pnpm/config.pick-registry-for-package@1100.0.3
  - @pnpm/network.auth-header@1100.0.2
  - @pnpm/resolving.registry.types@1100.0.3

## 1100.2.9

### Patch Changes

- Updated dependencies [e9e876c]
  - @pnpm/config.reader@1101.2.2

## 1100.2.8

### Patch Changes

- Updated dependencies [707a879]
  - @pnpm/config.reader@1101.2.1

## 1100.2.7

### Patch Changes

- Updated dependencies [8fdd9a9]
- Updated dependencies [5f34a8d]
- Updated dependencies [c969392]
- Updated dependencies [817b1b4]
- Updated dependencies [c969392]
- Updated dependencies [2de318b]
  - @pnpm/config.reader@1101.2.0

## 1100.2.6

### Patch Changes

- Updated dependencies [42a8f29]
  - @pnpm/config.reader@1101.1.4

## 1100.2.5

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/config.pick-registry-for-package@1100.0.2
  - @pnpm/resolving.registry.types@1100.0.2
  - @pnpm/config.reader@1101.1.3
  - @pnpm/network.fetch@1100.0.2
  - @pnpm/cli.utils@1101.0.2

## 1100.2.4

### Patch Changes

- @pnpm/cli.utils@1101.0.1

## 1100.2.3

### Patch Changes

- Updated dependencies [0fbcf74]
  - @pnpm/config.reader@1101.1.2

## 1100.2.2

### Patch Changes

- @pnpm/config.reader@1101.1.1

## 1100.2.1

### Patch Changes

- Updated dependencies [7d25bc1]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0

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
