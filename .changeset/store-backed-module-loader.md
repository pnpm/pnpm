---
"@pnpm/esm-loader": minor
---

Added an experimental Node.js loader that reads ESM, CommonJS, and JSON modules directly from the content-addressable store using an explicit dependency manifest. Applications can load compatible packages without materializing package directories.

Packages can opt out of CAS loading by using a normal global virtual store installation with their dependency trees.
