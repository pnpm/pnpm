---
"@pnpm/plugin-commands-listing": patch
"@pnpm/list": patch
"pnpm": patch
---

Fix issue where `pnpm list --json pkg` shows `"private": false` for a private package [#8519](https://github.com/pnpm/pnpm/issues/8519).
