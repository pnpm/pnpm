# @pnpm/engine.pm.commands

## 1101.1.9

### Patch Changes

- Updated dependencies [e9e876c]
- Updated dependencies [15e9e35]
  - @pnpm/config.reader@1101.2.2
  - @pnpm/resolving.npm-resolver@1101.0.3
  - @pnpm/installing.client@1100.0.12
  - @pnpm/installing.deps-restorer@1101.0.8
  - @pnpm/store.connection-manager@1100.0.13
  - @pnpm/store.controller@1101.0.4
  - @pnpm/global.commands@1100.0.14
  - @pnpm/installing.env-installer@1101.0.7

## 1101.1.8

### Patch Changes

- @pnpm/deps.graph-hasher@1100.1.4
- @pnpm/installing.deps-restorer@1101.0.7
- @pnpm/installing.env-installer@1101.0.6
- @pnpm/lockfile.fs@1100.0.6
- @pnpm/installing.client@1100.0.11
- @pnpm/global.commands@1100.0.13
- @pnpm/store.connection-manager@1100.0.12
- @pnpm/store.controller@1101.0.3

## 1101.1.7

### Patch Changes

- d0982fc: Fixed the `pn`, `pnpx`, and `pnx` aliases failing in Git Bash / MSYS2 on Windows when pnpm was installed via `@pnpm/exe` (or after `pnpm self-update`) [#11486](https://github.com/pnpm/pnpm/issues/11486). Running `pnpx` (or `pnx`) printed the cmd.exe banner and dropped the user into an interactive command prompt instead of running `pnpm dlx`. The `bin` field rewrite on Windows was pointing those aliases at `.cmd` files; cmd-shim's Bash shim for a `.cmd` target wraps it in `exec cmd /C ...`, and MSYS2 mangles `/C` into a Windows path before cmd.exe sees it. The aliases are now `.exe` hardlinks of the SEA binary, which detects which name it was launched as via `process.execPath` and prepends `dlx` for `pnpx` / `pnx`.
- Updated dependencies [12313f1]
- Updated dependencies [27425d7]
- Updated dependencies [707a879]
  - @pnpm/installing.deps-restorer@1101.0.6
  - @pnpm/lockfile.fs@1100.0.5
  - @pnpm/lockfile.types@1100.0.4
  - @pnpm/config.reader@1101.2.1
  - @pnpm/global.commands@1100.0.12
  - @pnpm/installing.client@1100.0.10
  - @pnpm/store.controller@1101.0.3
  - @pnpm/installing.env-installer@1101.0.5
  - @pnpm/deps.graph-hasher@1100.1.3
  - @pnpm/resolving.npm-resolver@1101.0.2
  - @pnpm/store.connection-manager@1100.0.11

## 1101.1.6

### Patch Changes

- 0219ab2: Fixed `pnpm self-update` on installations originally set up by pnpm v10. v10 added `PNPM_HOME` directly to PATH and wrote a `pnpm` bootstrap shim there. v11 setup writes shims under `PNPM_HOME/bin` instead, so when a v10 user upgrades to v11 the legacy shim at `PNPM_HOME` keeps pointing into the old `.tools/<version>` install — `pnpm --version` continues to report the pre-update version even though the new version was installed under `global/v11`. Self-update now detects this layout, refreshes the legacy shims so the upgrade actually takes effect, and prints a hint suggesting `pnpm setup` to migrate PATH to the v11 layout. [#11464](https://github.com/pnpm/pnpm/issues/11464).
- Updated dependencies [8fdd9a9]
- Updated dependencies [5f34a8d]
- Updated dependencies [c969392]
- Updated dependencies [ab6c42d]
- Updated dependencies [817b1b4]
- Updated dependencies [c969392]
- Updated dependencies [2de318b]
  - @pnpm/config.reader@1101.2.0
  - @pnpm/building.policy@1100.0.3
  - @pnpm/installing.deps-restorer@1101.0.5
  - @pnpm/global.commands@1100.0.11
  - @pnpm/store.connection-manager@1100.0.10
  - @pnpm/installing.client@1100.0.9
  - @pnpm/installing.env-installer@1101.0.4
  - @pnpm/store.controller@1101.0.2

## 1101.1.5

### Patch Changes

- Updated dependencies [72629fc]
  - @pnpm/global.commands@1100.0.10

## 1101.1.4

### Patch Changes

- c1d29d2: `pnpm self-update` (with no version argument) no longer downgrades pnpm when the registry's `latest` dist-tag points to an older release than the currently active version. Run `pnpm self-update latest` to force a downgrade [#11418](https://github.com/pnpm/pnpm/issues/11418).
- Updated dependencies [42a8f29]
  - @pnpm/config.reader@1101.1.4
  - @pnpm/global.commands@1100.0.9
  - @pnpm/store.connection-manager@1100.0.9
  - @pnpm/installing.deps-restorer@1101.0.4
  - @pnpm/installing.client@1100.0.8
  - @pnpm/store.controller@1101.0.2
  - @pnpm/installing.env-installer@1101.0.3

## 1101.1.3

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/workspace.project-manifest-reader@1100.0.3
  - @pnpm/installing.deps-restorer@1101.0.3
  - @pnpm/store.connection-manager@1100.0.8
  - @pnpm/resolving.npm-resolver@1101.0.1
  - @pnpm/deps.graph-hasher@1100.1.2
  - @pnpm/installing.client@1100.0.7
  - @pnpm/store.controller@1101.0.2
  - @pnpm/building.policy@1100.0.2
  - @pnpm/config.reader@1101.1.3
  - @pnpm/bins.linker@1100.0.3
  - @pnpm/shell.path@1100.0.1
  - @pnpm/cli.utils@1101.0.2
  - @pnpm/cli.meta@1100.0.2
  - @pnpm/installing.env-installer@1101.0.3
  - @pnpm/global.commands@1100.0.8
  - @pnpm/lockfile.types@1100.0.3
  - @pnpm/global.packages@1100.0.2

## 1101.1.2

### Patch Changes

- Updated dependencies [685a369]
  - @pnpm/installing.deps-restorer@1101.0.2
  - @pnpm/global.commands@1100.0.7
  - @pnpm/cli.utils@1101.0.1
  - @pnpm/store.controller@1101.0.1
  - @pnpm/installing.env-installer@1101.0.2
  - @pnpm/store.connection-manager@1100.0.7

## 1101.1.1

### Patch Changes

- 0fbcf74: `pnpm self-update` now keeps `package.json`'s `packageManager` and `devEngines.packageManager` in sync. When the legacy `packageManager` field pins pnpm, both fields are rewritten to the new exact pnpm version on update — `packageManager` to `pnpm@<version>` (without an integrity hash), and `devEngines.packageManager.version` to the same exact `<version>` (dropping any range operator). When only `devEngines.packageManager` is declared, the existing range-preserving behavior is unchanged [#11388](https://github.com/pnpm/pnpm/issues/11388).
- Updated dependencies [0fbcf74]
  - @pnpm/config.reader@1101.1.2
  - @pnpm/global.commands@1100.0.6
  - @pnpm/store.connection-manager@1100.0.6
  - @pnpm/installing.deps-restorer@1101.0.1
  - @pnpm/installing.client@1100.0.6
  - @pnpm/installing.env-installer@1101.0.1
  - @pnpm/store.controller@1101.0.0

## 1101.1.0

### Minor Changes

- 390b9d1: `pnpm self-update` now prints progress messages so the command isn't silent: `Checking for updates...` before resolving, `Updating pnpm from vX to vY...` once a newer version is found, and `Successfully updated pnpm to vY` on completion.

### Patch Changes

- @pnpm/installing.client@1100.0.5
- @pnpm/global.commands@1100.0.5
- @pnpm/store.connection-manager@1100.0.5
- @pnpm/store.controller@1101.0.0
- @pnpm/installing.deps-restorer@1101.0.0
- @pnpm/installing.env-installer@1101.0.0

## 1101.0.2

### Patch Changes

- ef4ef7b: Restored the legacy `@pnpm/{macos,win,linux,linuxstatic}-{x64,arm64}` npm names for the platform-specific optional dependencies of `@pnpm/exe`, reverting the scope-nested `@pnpm/exe.<platform>-<arch>[-musl]` rename from [#11316](https://github.com/pnpm/pnpm/pull/11316) on the published package names only — the workspace directory layout (`pnpm/artifacts/<platform>-<arch>[-musl]/`) and the GitHub release asset filenames stay on the new scheme. The rename broke `pnpm self-update` from v10, which looks up the platform child by its legacy name. `linkExePlatformBinary` now checks for both schemes so a later rename can ship without a v10-compatibility hazard.
  - @pnpm/installing.client@1100.0.4
  - @pnpm/store.controller@1101.0.0
  - @pnpm/installing.deps-restorer@1101.0.0
  - @pnpm/resolving.npm-resolver@1101.0.0
  - @pnpm/installing.env-installer@1101.0.0
  - @pnpm/store.connection-manager@1100.0.4
  - @pnpm/global.commands@1100.0.4
  - @pnpm/deps.graph-hasher@1100.1.1
  - @pnpm/config.reader@1101.1.1

## 1101.0.1

### Patch Changes

- Updated dependencies [7d25bc1]
- Updated dependencies [72c1e05]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0
  - @pnpm/deps.graph-hasher@1100.1.0
  - @pnpm/installing.deps-restorer@1100.0.3
  - @pnpm/resolving.npm-resolver@1100.1.0
  - @pnpm/store.connection-manager@1100.0.3
  - @pnpm/global.commands@1100.0.3
  - @pnpm/installing.client@1100.0.3
  - @pnpm/installing.env-installer@1100.1.1
  - @pnpm/lockfile.types@1100.0.2
  - @pnpm/store.controller@1100.0.2

## 1101.0.0

### Major Changes

- cee550a: **Breaking:** removed the `managePackageManagerVersions`, `packageManagerStrict`, and `packageManagerStrictVersion` settings. They existed only to derive the `onFail` behavior for the legacy `packageManager` field, and the `pmOnFail` setting introduced alongside `pnpm with` subsumes all three — it directly sets the `onFail` behavior of both `packageManager` and `devEngines.packageManager`. The `COREPACK_ENABLE_STRICT` environment variable is no longer honored (it only gated `packageManagerStrict`); use `pmOnFail` instead.

  Migration:

  | Removed setting                       | Replace with                   |
  | ------------------------------------- | ------------------------------ |
  | `managePackageManagerVersions: true`  | `pmOnFail: download` (default) |
  | `managePackageManagerVersions: false` | `pmOnFail: ignore`             |
  | `packageManagerStrict: false`         | `pmOnFail: warn`               |
  | `packageManagerStrictVersion: true`   | `pmOnFail: error`              |
  | `COREPACK_ENABLE_STRICT=0`            | `pmOnFail: warn`               |

### Minor Changes

- 9af708a: Add `pnpm with <version|current> <args...>` command. Runs pnpm at a specific version (or the currently active one) for a single invocation, bypassing the project's `packageManager` and `devEngines.packageManager` pins. Uses the same install mechanism as `pnpm self-update`, caching the downloaded pnpm in the global virtual store for reuse.

  Examples:

  ```
  pnpm with current install           # ignore the pinned version, use the running pnpm
  pnpm with 11.0.0-rc.1 install       # install using pnpm 11.0.0-rc.1
  pnpm with next install              # install using the "next" dist-tag
  ```

  Also adds a new `pmOnFail` setting that overrides the `onFail` behavior of `packageManager` and `devEngines.packageManager`. Accepted values: `download`, `error`, `warn`, `ignore`. Can be set via CLI flag, env var, `pnpm-workspace.yaml`, or `.npmrc` — useful when version management is handled by an external tool (asdf, mise, Volta, etc.) and the project wants pnpm itself to skip the check.

  ```
  pnpm install --pm-on-fail=ignore            # direct CLI flag
  pnpm_config_pm_on_fail=ignore pnpm install  # env var
  # or in pnpm-workspace.yaml:
  #   pmOnFail: ignore
  ```

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/cli.utils@1101.0.0
  - @pnpm/config.reader@1101.0.0
  - @pnpm/installing.env-installer@1100.1.0
  - @pnpm/global.commands@1100.0.2
  - @pnpm/store.connection-manager@1100.0.2
  - @pnpm/bins.linker@1100.0.2
  - @pnpm/workspace.project-manifest-reader@1100.0.2
  - @pnpm/installing.deps-restorer@1100.0.2
  - @pnpm/installing.client@1100.0.2
  - @pnpm/store.controller@1100.0.1

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
