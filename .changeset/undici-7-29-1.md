---
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/fetching.pick-fetcher": patch
"@pnpm/network.fetch": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Updated `undici` to 7.29.1, which fixes a denial of service in WebSocket decompression [GHSA-3wwx-pv8p-q78v](https://github.com/advisories/GHSA-3wwx-pv8p-q78v).
