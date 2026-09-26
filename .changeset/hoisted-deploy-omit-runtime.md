---
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` with `node-linker=hoisted` omits downloaded runtimes from the deployed `node_modules`. That includes the Node.js runtime downloaded for `engines.runtime` when `onFail` is `download` [pnpm/pnpm#12468](https://github.com/pnpm/pnpm/issues/12468).
