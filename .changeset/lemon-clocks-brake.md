---
"@pnpm/config": minor
"@pnpm/core": minor
"@pnpm/headless": minor
---

nodeLinker may accept two new values: `isolated-node-modules` and `hoisted-node-modules`. `hoisted-node-modules` will create a "classic" `node_modules` folder without using symlinks.
