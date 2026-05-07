# @pnpm/exe

## 11.0.7

### Patch Changes

- fbeee82: Restore the execute bit on the `node-gyp` shims packed inside `@pnpm/exe` (`dist/node-gyp-bin/node-gyp`, `dist/node-gyp-bin/node-gyp.cmd`, and `dist/node_modules/node-gyp/bin/node-gyp.js`). Without this, `pnpm/action-setup`'s standalone path (used on runners with Node.js < 22.13) failed any install whose lifecycle script invoked `node-gyp rebuild` with `sh: 1: node-gyp: Permission denied` [#11483](https://github.com/pnpm/pnpm/issues/11483).

## 11.0.5

### Patch Changes

- 47b100e: Drop the `darwin-x64` artifact from `@pnpm/exe` and from the GitHub release page. The Node.js SEA mechanism `pnpm pack-app` uses produces a binary that segfaults at startup on Intel Macs because of an upstream Node.js bug ([nodejs/node#62893](https://github.com/nodejs/node/issues/62893), tracked alongside [#59553](https://github.com/nodejs/node/issues/59553); the Node.js team has [opted not to fix it](https://github.com/nodejs/node/pull/60250) on the grounds that x64 macOS is being phased out). Re-signing with `codesign` or `ldid` doesn't help — the corruption is in LIEF's Mach-O surgery, before signing.

  Intel Mac users should install pnpm via `npm install -g pnpm` (uses the system Node.js, no SEA), or stay on pnpm 10.x. `@pnpm/exe`'s preinstall on Intel Mac now exits with a clear error pointing at these alternatives.

  Closes [#11423](https://github.com/pnpm/pnpm/issues/11423).

## 11.0.3

### Patch Changes

- a99ffe0: Also pass `verbatimSymlinks: true` to the `fs.cpSync` call in `__utils__/scripts/src/copy-artifacts.ts`, which is the script that actually produces the GitHub release tarballs (`pnpm-{darwin,linux}-{x64,arm64}.tar.gz`). The previous fix in #11399 only covered the `fs.cpSync` in `pnpm/artifacts/exe/scripts/build-artifacts.ts`, which packages the `dist/` shipped inside the npm-published `@pnpm/exe` package. Verified by inspecting the v11.0.2 release tarballs after #11399 landed: the broken `/home/runner/work/pnpm/pnpm/...` symlinks under `dist/node_modules/.bin/` were still present, confirming `copy-artifacts.ts` is the offender for the GitHub release path. Follow-up to #11398.

## 11.0.2

### Patch Changes

- d613c81: Preserve relative symlinks under `dist/node_modules/.bin/` when copying `dist/` for the standalone executable artifact, by passing `verbatimSymlinks: true` to `fs.cpSync`. This stops the release tarballs from baking absolute paths to the build host (e.g. `/home/runner/work/pnpm/pnpm/...`) into symlink targets, which previously made the tarballs unextractable by strict tar implementations that validate symlink targets (e.g. hermit) [#11398](https://github.com/pnpm/pnpm/issues/11398).

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
