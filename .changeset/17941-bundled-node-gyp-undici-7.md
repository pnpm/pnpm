---
"pacquet": patch
"pnpm": patch
---

The bundled `node-gyp` now uses undici 7, which has the fix for CVE-2026-84947. Undici 6 has no fixed release. On Node.js 26, `node-gyp` downloads through a proxy or with a custom CA file no longer crash with "invalid onError method" [pnpm/pnpm#17941](https://github.com/pnpm/pnpm/issues/17941).
