---
"pnpm": patch
"@pnpm/plugin-commands-script-runners": patch
---

`pnpm dlx` should ignore any settings that are in a `package.json` file found in the current working directory [#7198](https://github.com/pnpm/pnpm/issues/7198).
