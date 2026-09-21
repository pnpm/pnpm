---
"pacquet": patch
---

`pnpm install` now reuses a tarball download that is already in flight when another resolution of the same archive still needs its package.json [#15037](https://github.com/pnpm/pnpm/issues/15037).
