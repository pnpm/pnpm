---
"@pnpm/network.fetch": patch
"pnpm": patch
---

A registry response with too many content encodings can no longer crash pnpm. pnpm now uses undici 7.30.1, which fixes [GHSA-xjj6-f33q-929c](https://github.com/advisories/GHSA-xjj6-f33q-929c).
