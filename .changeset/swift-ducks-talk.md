---
"@pnpm/lifecycle": patch
"pnpm": patch
---

Do not run node-gyp rebuild if `preinstall` lifecycle script is present [#7206](https://github.com/pnpm/pnpm/pull/7206).
