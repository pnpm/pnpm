---
"@pnpm/network.fetch": patch
"pnpm": patch
---

Fixed a denial-of-service vulnerability in HTTP requests triggered by excessive content encodings in a server response [GHSA-xjj6-f33q-929c](https://github.com/advisories/GHSA-xjj6-f33q-929c).
