---
"@pnpm/pnpr": patch
---

pnpr logs scoped package URLs such as `https://registry.npmjs.org/@babel/cli` with their registry host intact. It previously logged them as `https://redacted@babel/cli`.
