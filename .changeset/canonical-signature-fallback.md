---
"@pnpm/tools.plugin-commands-self-updater": patch
"pnpm": patch
---

Switching to the pnpm version in `packageManager` now works through registry mirrors that strip npm's signatures or re-sign packages with their own key. pnpm then reads the signature from `https://registry.npmjs.org/` and verifies it against the integrity of the downloaded pnpm [#16778](https://github.com/pnpm/pnpm/issues/16778).
