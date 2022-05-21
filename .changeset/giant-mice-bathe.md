---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

When `auto-install-peers` is set to `true`, automatically install missing peer dependencies without writing them to `package.json` as dependencies. This makes pnpm handle peer dependencies the same way as npm v7 [#4776](https://github.com/pnpm/pnpm/pull/4776).
