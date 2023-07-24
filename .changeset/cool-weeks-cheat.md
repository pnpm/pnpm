---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

Fix a bug where recursive run does not fail when there are no packages have the script and there is a root `package.json` [#6844](https://github.com/pnpm/pnpm/issues/6844)
