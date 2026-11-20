---
"@pnpm/store.pkg-finder": minor
"@pnpm/deps.compliance.license-scanner": patch
"pnpm": patch
---

`pnpm licenses` reads each package's file index from the store once, instead of once per peer-dependency variant of the package.
