---
"@pnpm/installing.commands": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` with `nodeLinker: hoisted` no longer copies downloaded runtimes, such as the Node.js runtime from `engines.runtime`, into the deployed `node_modules` [pnpm/pnpm#12468](https://github.com/pnpm/pnpm/issues/12468).
