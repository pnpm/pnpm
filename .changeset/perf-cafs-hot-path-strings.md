---
"@pnpm/store.cafs": patch
"pnpm": patch
---

Optimize hot path string operations in content-addressable store: replace `path.join` with string concatenation in `contentPathFromHex` and `getFilePathByModeInCafs` (~30k calls per install), and increase `gunzipSync` chunk size to 128KB for fewer buffer allocations during tarball decompression.
