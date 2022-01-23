---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

`pnpm exec` should look for the executed command in the `node_modules/.bin` directory that is relative to the current working directory. Only after that should it look for the executable in the workspace root.
