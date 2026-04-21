---
"@pnpm/engine.pm.commands": patch
"@pnpm/exe": patch
"pnpm": patch
---

Restored the legacy `@pnpm/{macos,win,linux,linuxstatic}-{x64,arm64}` npm names for the platform-specific optional dependencies of `@pnpm/exe`, reverting the scope-nested `@pnpm/exe.<platform>-<arch>[-musl]` rename from [#11316](https://github.com/pnpm/pnpm/pull/11316) on the published package names only — the workspace directory layout (`pnpm/artifacts/<platform>-<arch>[-musl]/`) and the GitHub release asset filenames stay on the new scheme. The rename broke `pnpm self-update` from v10, which looks up the platform child by its legacy name. `linkExePlatformBinary` now checks for both schemes so a later rename can ship without a v10-compatibility hazard.
