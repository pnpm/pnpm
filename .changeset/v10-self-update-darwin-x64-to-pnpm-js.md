---
"@pnpm/tools.plugin-commands-self-updater": patch
"pnpm": patch
---

When self-updating from v10's `@pnpm/exe` to v11+ on Intel macOS (darwin-x64), `pnpm self-update` now transparently switches to the JS-only `pnpm` package on npm instead of installing `@pnpm/exe@v11+` (which doesn't ship a working binary for Intel Macs because of an upstream Node.js SEA bug — see [#11423](https://github.com/pnpm/pnpm/issues/11423) and [nodejs/node#62893](https://github.com/nodejs/node/issues/62893)). Without this, the self-update would silently leave the user with no working `pnpm` binary. The new install requires Node.js to be available on `PATH`; a warning is printed when the swap happens. All other host/version combinations are unchanged.
