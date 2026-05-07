# @pnpm/global.commands

## 1100.0.13

### Patch Changes

- @pnpm/installing.deps-installer@1101.0.8
- @pnpm/deps.inspection.list@1100.0.7
- @pnpm/store.connection-manager@1100.0.12

## 1100.0.12

### Patch Changes

- Updated dependencies [12313f1]
- Updated dependencies [27425d7]
- Updated dependencies [707a879]
  - @pnpm/installing.deps-installer@1101.0.7
  - @pnpm/config.reader@1101.2.1
  - @pnpm/deps.inspection.list@1100.0.6
  - @pnpm/store.connection-manager@1100.0.11

## 1100.0.11

### Patch Changes

- Updated dependencies [8fdd9a9]
- Updated dependencies [5f34a8d]
- Updated dependencies [c969392]
- Updated dependencies [ab6c42d]
- Updated dependencies [817b1b4]
- Updated dependencies [c969392]
- Updated dependencies [2de318b]
  - @pnpm/config.reader@1101.2.0
  - @pnpm/installing.deps-installer@1101.0.6
  - @pnpm/store.connection-manager@1100.0.10

## 1100.0.10

### Patch Changes

- 72629fc: Fix `pnpm -g ls --json` and `pnpm -g ls --parseable` so they emit valid JSON and parseable output respectively, matching pnpm 10 behavior. Since the isolated global packages refactor in pnpm 11, the global list command had a custom path that always printed plain text and ignored `--json`/`--parseable`, which broke tools like `npm-check-updates` that parse the JSON output [#11440](https://github.com/pnpm/pnpm/issues/11440).

  `pnpm -g ls --depth=<n>` (with n > 0) now errors when more than one isolated global install would be involved, since each install has its own lockfile and merging their transitive trees would be incoherent. When the request can be narrowed to a single install group, the regular `list` flow is used and the full dependency tree is shown.

## 1100.0.9

### Patch Changes

- Updated dependencies [42a8f29]
  - @pnpm/config.reader@1101.1.4
  - @pnpm/store.connection-manager@1100.0.9
  - @pnpm/installing.deps-installer@1101.0.5

## 1100.0.8

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/installing.deps-installer@1101.0.4
  - @pnpm/store.connection-manager@1100.0.8
  - @pnpm/pkg-manifest.reader@1100.0.2
  - @pnpm/config.matcher@1100.0.1
  - @pnpm/bins.resolver@1100.0.2
  - @pnpm/config.reader@1101.1.3
  - @pnpm/bins.remover@1100.0.2
  - @pnpm/bins.linker@1100.0.3
  - @pnpm/cli.command@1100.0.1
  - @pnpm/cli.utils@1101.0.2
  - @pnpm/global.packages@1100.0.2

## 1100.0.7

### Patch Changes

- 685a369: Fix `ENOENT` symlink failure when `pnpm add -g` triggers the approve-builds prompt. The global add flow used to forward an absolute `modulesDir` (`<installDir>/node_modules`) into the install run by `approve-builds`. The install layer treated `modulesDir` as a path relative to `lockfileDir` and joined it again, producing a doubled path on Windows because `path.join` does not collapse an embedded absolute path. The hoist step then tried to `mkdir` and symlink under `<installDir>\<installDir>\node_modules\.pnpm\node_modules\...` and failed with `ENOENT` [#11403](https://github.com/pnpm/pnpm/issues/11403).
  - @pnpm/cli.utils@1101.0.1
  - @pnpm/installing.deps-installer@1101.0.3
  - @pnpm/store.connection-manager@1100.0.7

## 1100.0.6

### Patch Changes

- Updated dependencies [0fbcf74]
  - @pnpm/config.reader@1101.1.2
  - @pnpm/store.connection-manager@1100.0.6
  - @pnpm/installing.deps-installer@1101.0.2

## 1100.0.5

### Patch Changes

- Updated dependencies [005eeb7]
  - @pnpm/installing.deps-installer@1101.0.1
  - @pnpm/store.connection-manager@1100.0.5

## 1100.0.4

### Patch Changes

- @pnpm/installing.deps-installer@1101.0.0
- @pnpm/store.connection-manager@1100.0.4
- @pnpm/config.reader@1101.1.1

## 1100.0.3

### Patch Changes

- Updated dependencies [7d25bc1]
- Updated dependencies [72c1e05]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0
  - @pnpm/installing.deps-installer@1100.0.3
  - @pnpm/store.connection-manager@1100.0.3

## 1100.0.2

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/cli.utils@1101.0.0
  - @pnpm/config.reader@1101.0.0
  - @pnpm/store.connection-manager@1100.0.2
  - @pnpm/bins.linker@1100.0.2
  - @pnpm/installing.deps-installer@1100.0.2

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/bins.linker@1100.0.1
  - @pnpm/bins.remover@1100.0.1
  - @pnpm/bins.resolver@1100.0.1
  - @pnpm/cli.utils@1100.0.1
  - @pnpm/config.reader@1100.0.1
  - @pnpm/global.packages@1100.0.1
  - @pnpm/installing.deps-installer@1100.0.1
  - @pnpm/pkg-manifest.reader@1100.0.1
  - @pnpm/store.connection-manager@1100.0.1

## 1000.0.0

### Minor Changes

- fd511e4: Isolated global packages. Each globally installed package (or group of packages installed together) now gets its own isolated installation directory with its own `package.json`, `node_modules/`, and lockfile. This prevents global packages from interfering with each other through peer dependency conflicts, hoisting changes, or version resolution shifts.

  Key changes:

  - `pnpm add -g <pkg>` creates an isolated installation in `{pnpmHomeDir}/global/v11/{hash}/`
  - `pnpm remove -g <pkg>` removes the entire installation group containing the package
  - `pnpm update -g [pkg]` re-installs packages in new isolated directories
  - `pnpm list -g` scans isolated directories to show all installed global packages
  - `pnpm install -g` (no args) is no longer supported; use `pnpm add -g <pkg>` instead

### Patch Changes

- Updated dependencies [7730a7f]
- Updated dependencies [5f73b0f]
- Updated dependencies [449dacf]
- Updated dependencies [996284f]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [9b0a460]
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
- Updated dependencies [05fb1ae]
- Updated dependencies [da2429d]
- Updated dependencies [1cc61e8]
- Updated dependencies [491a84f]
- Updated dependencies [9b801c8]
- Updated dependencies [13855ac]
- Updated dependencies [62f760e]
- Updated dependencies [f0ae1b9]
- Updated dependencies [9fc552d]
- Updated dependencies [394d88c]
- Updated dependencies [6e9cad3]
- Updated dependencies [672e58c]
- Updated dependencies [312226c]
- Updated dependencies [cb228c9]
- Updated dependencies [2fc9139]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [075aa99]
- Updated dependencies [fd511e4]
- Updated dependencies [ae43ac7]
- Updated dependencies [d7b8be4]
- Updated dependencies [ccec8e7]
- Updated dependencies [ba065f6]
- Updated dependencies [4158906]
- Updated dependencies [ac944ef]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [96704a1]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [69ebe38]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [f40177f]
- Updated dependencies [05158d2]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [38b8e35]
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
- Updated dependencies [9587dac]
- Updated dependencies [09a999a]
- Updated dependencies [559f903]
- Updated dependencies [41dc031]
- Updated dependencies [3574905]
- Updated dependencies [4362c06]
  - @pnpm/config.reader@1005.0.0
  - @pnpm/installing.deps-installer@1013.0.0
  - @pnpm/bins.resolver@1001.0.0
  - @pnpm/bins.linker@1001.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/cli.utils@1002.0.0
  - @pnpm/pkg-manifest.reader@1001.0.0
  - @pnpm/store.connection-manager@1003.0.0
  - @pnpm/bins.remover@1001.0.0
  - @pnpm/config.matcher@1001.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/cli.command@1001.0.0
  - @pnpm/global.packages@1000.0.0
