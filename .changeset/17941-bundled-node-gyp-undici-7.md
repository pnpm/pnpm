---
"pacquet": patch
"pnpm": patch
---

The `node-gyp` bundled with pnpm no longer ships undici 6, which security scanners flag for CVE-2026-84947. On Node.js 26, `node-gyp` downloads through a proxy or with a custom CA file no longer crash with "invalid onError method" [pnpm/pnpm#17941](https://github.com/pnpm/pnpm/issues/17941).
