---
"@pnpm/fetching.binary-fetcher": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

pnpm no longer crashes on startup when the temporary directory set by `TMPDIR`, `TEMP`, or `TMP` does not exist [#4960](https://github.com/pnpm/pnpm/issues/4960).
