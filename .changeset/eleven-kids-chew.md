---
"@pnpm/store-path": major
"pnpm": major
---

Changed the location of the global store from `~/.pnpm-store` to `<pnpm home directory>/store`

On Linux, by default it will be `~/.local/share/pnpm/store`
On Windows: `%LOCALAPPDATA%/pnpm/store`
On macOS: `~/Library/pnpm/store`

Related issue: [#2574](https://github.com/pnpm/pnpm/issues/2574)
