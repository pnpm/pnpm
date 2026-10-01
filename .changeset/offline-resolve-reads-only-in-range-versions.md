---
"pacquet": patch
---

Sped up `pnpm install --offline` when the version a range picks is not in the store. While looking for a store-held version, pnpm now reads only the versions the range admits, not every version of the package [#ISSUE](https://github.com/pnpm/pnpm/issues/ISSUE).
