---
"@pnpm/exe": major
"pnpm": minor
---

Renamed the platform-specific optional dependencies of `@pnpm/exe` to the new `@pnpm/exe.<platform>-<arch>[-<libc>]` scheme, using `process.platform` values (`linux`, `darwin`, `win32`) for the OS segment. The umbrella package `@pnpm/exe` itself is unchanged so existing `npm i -g @pnpm/exe` and `pnpm self-update` flows keep working.

| before | after |
| --- | --- |
| `@pnpm/linux-x64` | `@pnpm/exe.linux-x64` |
| `@pnpm/linux-arm64` | `@pnpm/exe.linux-arm64` |
| `@pnpm/linuxstatic-x64` | `@pnpm/exe.linux-x64-musl` |
| `@pnpm/linuxstatic-arm64` | `@pnpm/exe.linux-arm64-musl` |
| `@pnpm/macos-x64` | `@pnpm/exe.darwin-x64` |
| `@pnpm/macos-arm64` | `@pnpm/exe.darwin-arm64` |
| `@pnpm/win-x64` | `@pnpm/exe.win32-x64` |
| `@pnpm/win-arm64` | `@pnpm/exe.win32-arm64` |

GitHub release asset filenames follow the same scheme — `pnpm-linuxstatic-x64.tar.gz` becomes `pnpm-linux-x64-musl.tar.gz`, `pnpm-macos-*` becomes `pnpm-darwin-*`, `pnpm-win-*` becomes `pnpm-win32-*`. Anyone downloading releases directly needs to use the new filenames; `get.pnpm.io/install.sh` and `install.ps1` will be updated in lockstep to accept both schemes based on the requested version.

Resolves [#11314](https://github.com/pnpm/pnpm/issues/11314).
