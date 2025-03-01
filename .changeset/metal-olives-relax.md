---
"@pnpm/plugin-commands-deploy": patch
"@pnpm/types": patch
pnpm: patch
---

Fix `pnpm deploy` creating a `package.json` without the `imports` and `license` field [#9193](https://github.com/pnpm/pnpm/issues/9193).
