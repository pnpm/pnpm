---
"@pnpm/config.reader": patch
"pnpm": patch
---

`pnpm dlx` now inherits `nodeDownloadMirrors` from the workspace configuration when downloading Node.js runtimes [#11281](https://github.com/pnpm/pnpm/issues/11281).
