---
"pacquet": patch
---

Sped up `pnpm install --offline` when the version a range picks is not in the store. While looking for a store-held version, pnpm now reads only the versions the range admits, not every version of the package [#16495](https://github.com/pnpm/pnpm/issues/16495).
