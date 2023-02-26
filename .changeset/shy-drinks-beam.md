---
"@pnpm/plugin-commands-env": patch
"pnpm": patch
---

`pnpm env -g` should fail with a meaningful error message if pnpm cannot find the pnpm home directory, which is the directory into which Node.js is installed.
