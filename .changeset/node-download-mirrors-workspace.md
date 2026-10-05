---
"@pnpm/config.reader": patch
"pnpm": patch
---

`pnpm dlx` now inherits the `release` entry of `nodeDownloadMirrors` from the workspace configuration when downloading Node.js runtimes [#11281](https://github.com/pnpm/pnpm/issues/11281). Other channels are not inherited: Node signs `SHASUMS256.txt` for `release` only, so a workspace-supplied `rc`/`nightly` mirror would provide both the archive and its checksum for a runtime that `dlx` executes. Mirrors the user configured globally for those channels continue to apply.
