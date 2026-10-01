---
"pacquet": patch
---

Sped up `pnpm install --offline` when the version a range picks is not in the store. While it looks for a version the store holds, pnpm now reads only the versions the range admits [#16495](https://github.com/pnpm/pnpm/issues/16495).
