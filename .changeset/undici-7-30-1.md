---
"@pnpm/network.fetch": patch
"pnpm": patch
---

Updated undici to 7.30.1, which fixes a denial of service through a response with too many content encodings ([GHSA-xjj6-f33q-929c](https://github.com/advisories/GHSA-xjj6-f33q-929c)) and a Subresource Integrity bypass through non-lowercase algorithm names ([GHSA-f72v-6w45-q472](https://github.com/advisories/GHSA-f72v-6w45-q472)).
