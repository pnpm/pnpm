# @pnpm/deps.inspection.commands

## 1100.1.3

### Patch Changes

- @pnpm/resolving.default-resolver@1100.0.4
- @pnpm/resolving.npm-resolver@1101.0.0
- @pnpm/deps.inspection.outdated@1100.0.4
- @pnpm/deps.inspection.list@1100.0.4
- @pnpm/global.commands@1100.0.4
- @pnpm/lockfile.fs@1100.0.3
- @pnpm/config.reader@1101.1.1
- @pnpm/deps.inspection.peers-checker@1100.0.3

## 1100.1.2

### Patch Changes

- Updated dependencies [7d25bc1]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0
  - @pnpm/resolving.npm-resolver@1100.1.0
  - @pnpm/deps.inspection.outdated@1100.0.3
  - @pnpm/global.commands@1100.0.3
  - @pnpm/resolving.default-resolver@1100.0.3
  - @pnpm/lockfile.fs@1100.0.2
  - @pnpm/deps.inspection.list@1100.0.3
  - @pnpm/deps.inspection.peers-checker@1100.0.2

## 1100.1.1

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/cli.utils@1101.0.0
  - @pnpm/config.reader@1101.0.0
  - @pnpm/global.commands@1100.0.2
  - @pnpm/deps.inspection.outdated@1100.0.2
  - @pnpm/resolving.default-resolver@1100.0.2
  - @pnpm/deps.inspection.list@1100.0.2

## 1100.1.0

### Minor Changes

- 2410cf4: Added the `pnpm docs` command and its alias `pnpm home`. This command opens the package documentation or homepage in the browser. When the package has no valid homepage, it falls back to `https://npmx.dev/package/<name>`.

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0
  - @pnpm/cli.utils@1100.0.1
  - @pnpm/config.pick-registry-for-package@1100.0.1
  - @pnpm/config.reader@1100.0.1
  - @pnpm/deps.inspection.list@1100.0.1
  - @pnpm/deps.inspection.outdated@1100.0.1
  - @pnpm/deps.inspection.peers-checker@1100.0.1
  - @pnpm/global.commands@1100.0.1
  - @pnpm/global.packages@1100.0.1
  - @pnpm/installing.modules-yaml@1100.0.1
  - @pnpm/lockfile.fs@1100.0.1
  - @pnpm/network.auth-header@1100.0.1
  - @pnpm/network.fetch@1100.0.1
  - @pnpm/resolving.default-resolver@1100.0.1
  - @pnpm/resolving.npm-resolver@1100.0.1
  - @pnpm/resolving.registry.types@1100.0.1

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

### Minor Changes

- fd511e4: Isolated global packages. Each globally installed package (or group of packages installed together) now gets its own isolated installation directory with its own `package.json`, `node_modules/`, and lockfile. This prevents global packages from interfering with each other through peer dependency conflicts, hoisting changes, or version resolution shifts.

  Key changes:

  - `pnpm add -g <pkg>` creates an isolated installation in `{pnpmHomeDir}/global/v11/{hash}/`
  - `pnpm remove -g <pkg>` removes the entire installation group containing the package
  - `pnpm update -g [pkg]` re-installs packages in new isolated directories
  - `pnpm list -g` scans isolated directories to show all installed global packages
  - `pnpm install -g` (no args) is no longer supported; use `pnpm add -g <pkg>` instead

- 263a8bc: Added `pnpm peers check` command that checks for unmet and missing peer dependency issues by reading the lockfile [#7087](https://github.com/pnpm/pnpm/issues/7087).
- d3d6938: Added native `pnpm view` command with `info`, `show`, and `v` aliases for viewing package information from the registry. Supports version ranges, dist-tags, aliases, field selection, and JSON output.
- 2464485: Added `--lockfile-only` option to `pnpm list` [#10020](https://github.com/pnpm/pnpm/issues/10020).
- 7d5ada0: `pnpm why` now shows a reverse dependency tree. The searched package appears at the root with its dependents as branches, walking back to workspace roots. This replaces the previous forward-tree output which was noisy and hard to read for deeply nested dependencies.

### Patch Changes

- 353bc16: Fix `pnpm list --only-projects` regression (fixes #10651). When `node_modules` is absent, fall back to the wanted lockfile so workspace projects are still listed. Ensure `--only-projects` is read from CLI options and restrict the dependency graph to workspace importers when the flag is set.
- 8b864cc: Show deprecation in table/list formats when latest version is deprecated [#8658](https://github.com/pnpm/pnpm/issues/8658).
- 8ffb1a7: `pnpm list` and `pnpm why` now display npm: protocol for aliased packages (e.g., `foo npm:is-odd@3.0.1`) [#8660](https://github.com/pnpm/pnpm/issues/8660).
- Updated dependencies [e1ea779]
- Updated dependencies [7116f35]
- Updated dependencies [7730a7f]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [3c72b6b]
- Updated dependencies [9f5c0e3]
- Updated dependencies [a297ebc]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [90bd3c3]
- Updated dependencies [3cfffaa]
- Updated dependencies [1cc61e8]
- Updated dependencies [606f53e]
- Updated dependencies [831f574]
- Updated dependencies [0e9c559]
- Updated dependencies [c7203b9]
- Updated dependencies [bb17724]
- Updated dependencies [05fb1ae]
- Updated dependencies [da2429d]
- Updated dependencies [1cc61e8]
- Updated dependencies [19f36cf]
- Updated dependencies [491a84f]
- Updated dependencies [fb8962f]
- Updated dependencies [ec7c5d7]
- Updated dependencies [f0ae1b9]
- Updated dependencies [a49b243]
- Updated dependencies [61cad0c]
- Updated dependencies [3417386]
- Updated dependencies [b1ad9c7]
- Updated dependencies [dcd16c7]
- Updated dependencies [19f36cf]
- Updated dependencies [861dd2a]
- Updated dependencies [0dfa8b8]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [075aa99]
- Updated dependencies [fd511e4]
- Updated dependencies [ae43ac7]
- Updated dependencies [ccec8e7]
- Updated dependencies [143ca78]
- Updated dependencies [263a8bc]
- Updated dependencies [4158906]
- Updated dependencies [6f361aa]
- Updated dependencies [ac944ef]
- Updated dependencies [0625e20]
- Updated dependencies [938ea1f]
- Updated dependencies [2464485]
- Updated dependencies [2cb0657]
- Updated dependencies [472d3af]
- Updated dependencies [bb8baa7]
- Updated dependencies [d458ab3]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [144ce0e]
- Updated dependencies [efb48dc]
- Updated dependencies [56a59df]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [96704a1]
- Updated dependencies [bb8baa7]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [6c480a4]
- Updated dependencies [7d5ada0]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [ba70035]
- Updated dependencies [3585d9a]
- Updated dependencies [38b8e35]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [ed1a7fe]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [5bf7768]
- Updated dependencies [3cfffaa]
- Updated dependencies [ae43ac7]
- Updated dependencies [a5fdbf9]
- Updated dependencies [9d3f00b]
- Updated dependencies [6557dc0]
- Updated dependencies [efb48dc]
- Updated dependencies [6b3d87a]
- Updated dependencies [9587dac]
- Updated dependencies [09a999a]
- Updated dependencies [559f903]
- Updated dependencies [3574905]
  - @pnpm/cli.common-cli-options-help@1001.0.0
  - @pnpm/deps.inspection.list@1001.0.0
  - @pnpm/config.reader@1005.0.0
  - @pnpm/resolving.npm-resolver@1005.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/lockfile.fs@1002.0.0
  - @pnpm/cli.utils@1002.0.0
  - @pnpm/installing.modules-yaml@1001.0.0
  - @pnpm/deps.inspection.outdated@1002.0.0
  - @pnpm/config.pick-registry-for-package@1001.0.0
  - @pnpm/resolving.default-resolver@1003.0.0
  - @pnpm/network.auth-header@1001.0.0
  - @pnpm/store.path@1001.0.0
  - @pnpm/config.matcher@1001.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/network.fetch@1001.0.0
  - @pnpm/cli.command@1001.0.0
  - @pnpm/global.packages@1000.0.0
  - @pnpm/global.commands@1000.0.0
  - @pnpm/deps.inspection.peers-checker@1000.1.0
