# @pnpm/exe

## 11.0.0

### Patch Changes

- ef4ef7b: Restored the legacy `@pnpm/{macos,win,linux,linuxstatic}-{x64,arm64}` npm names for the platform-specific optional dependencies of `@pnpm/exe`, reverting the scope-nested `@pnpm/exe.<platform>-<arch>[-musl]` rename from [#11316](https://github.com/pnpm/pnpm/pull/11316) on the published package names only — the workspace directory layout (`pnpm/artifacts/<platform>-<arch>[-musl]/`) and the GitHub release asset filenames stay on the new scheme. The rename broke `pnpm self-update` from v10, which looks up the platform child by its legacy name. `linkExePlatformBinary` now checks for both schemes so a later rename can ship without a v10-compatibility hazard.

## 11.0.0

### Major Changes

- 5a293d2: Renamed the platform-specific optional dependencies of `@pnpm/exe` to the new `@pnpm/exe.<platform>-<arch>[-<libc>]` scheme, using `process.platform` values (`linux`, `darwin`, `win32`) for the OS segment. The umbrella package `@pnpm/exe` itself is unchanged so existing `npm i -g @pnpm/exe` and `pnpm self-update` flows keep working.

  | before                    | after                        |
  | ------------------------- | ---------------------------- |
  | `@pnpm/linux-x64`         | `@pnpm/exe.linux-x64`        |
  | `@pnpm/linux-arm64`       | `@pnpm/exe.linux-arm64`      |
  | `@pnpm/linuxstatic-x64`   | `@pnpm/exe.linux-x64-musl`   |
  | `@pnpm/linuxstatic-arm64` | `@pnpm/exe.linux-arm64-musl` |
  | `@pnpm/macos-x64`         | `@pnpm/exe.darwin-x64`       |
  | `@pnpm/macos-arm64`       | `@pnpm/exe.darwin-arm64`     |
  | `@pnpm/win-x64`           | `@pnpm/exe.win32-x64`        |
  | `@pnpm/win-arm64`         | `@pnpm/exe.win32-arm64`      |

  GitHub release asset filenames follow the same scheme — `pnpm-linuxstatic-x64.tar.gz` becomes `pnpm-linux-x64-musl.tar.gz`, `pnpm-macos-*` becomes `pnpm-darwin-*`, `pnpm-win-*` becomes `pnpm-win32-*`. Anyone downloading releases directly needs to use the new filenames; `get.pnpm.io/install.sh` and `install.ps1` will be updated in lockstep to accept both schemes based on the requested version.

  Resolves [#11314](https://github.com/pnpm/pnpm/issues/11314).

## 11.0.0

### Major Changes

- 491a84f: This package is now pure ESM.

## 9.5.0

## 7.1.8

### Patch Changes

- 7fb1ac0e4: Fix pre-compiled pnpm binaries crashing when NODE_MODULES is set.

## 6.19.0

### Minor Changes

- b6d74c545: Allow a system's package manager to override pnpm's default settings
