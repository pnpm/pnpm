---
"pacquet": patch
---

`pnpm install` now reports "Already up to date" when local tarball dependencies have not changed. This avoids re-importing hoisted `node_modules` trees on repeat installs [#14495](https://github.com/pnpm/pnpm/issues/14495).
