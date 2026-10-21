---
"pacquet": patch
---

Under `nodeLinker: hoisted`, `pnpm install` now leaves a patched package that is already in place alone. The same holds for a package whose peers include a patched package, such as every package peering on a patched `react-native` [#17188](https://github.com/pnpm/pnpm/issues/17188).
