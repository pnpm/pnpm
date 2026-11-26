---
"pnpm": patch
---

`pnpm config get ca` and `pnpm config list` now show every certificate authority set with `ca=` or `ca[]=` lines in `.npmrc`, as a JSON array. They showed only the last one.
