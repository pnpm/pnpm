---
"pacquet": patch
---

`pnpm install` now warns when the project's Node.js or pnpm version does not match its `engines` field. With `engineStrict` enabled, the install fails [pnpm/pnpm#17751](https://github.com/pnpm/pnpm/issues/17751).
