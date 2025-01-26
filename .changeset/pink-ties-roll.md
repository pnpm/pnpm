---
"@pnpm/plugin-commands-licenses": patch
"@pnpm/plugin-commands-outdated": patch
"@pnpm/plugin-commands-listing": patch
"@pnpm/plugin-commands-audit": patch
"@pnpm/render-peer-issues": patch
"@pnpm/dedupe.issues-renderer": patch
"@pnpm/default-reporter": patch
"pnpm": patch
---

Replace `strip-ansi` with the built-in `util.stripVTControlCharacters` [#9009](https://github.com/pnpm/pnpm/pull/9009).
