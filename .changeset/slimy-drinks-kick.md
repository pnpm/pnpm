---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Avoid re-importing package files into an already-populated global virtual store during warm installs.
