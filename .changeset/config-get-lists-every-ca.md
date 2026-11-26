---
"pnpm": patch
---

`pnpm config get ca` and `pnpm config list` now show every certificate authority set with `ca=` or `ca[]=` lines in `.npmrc`. Several certificate authorities are printed as a JSON array, and a single one is still printed as a string. They showed only the last one.
