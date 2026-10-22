---
"@pnpm/exe": patch
"pnpm": patch
---

The standalone pnpm executables now embed Node.js 24.22.1. They embedded Node.js 20, which has reached end of life and still trusts the certificate authority intermediates that Node.js distrusted in October 2026.
