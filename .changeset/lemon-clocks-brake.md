---
"@pnpm/config": minor
"@pnpm/core": minor
"@pnpm/headless": minor
---

nodeLinker may accept two new values: `isolated` and `hoisted`.

`hoisted` will create a "classic" `node_modules` folder without using symlinks.

`isolated` will be the default value that creates a symlinked `node_modules`.
