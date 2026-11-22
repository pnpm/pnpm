---
"pacquet": patch
---

Fixed `pnpm update` overwriting Node.js version ranges such as `>=20 <23` in `engines.runtime` and `devEngines.runtime` with an exact version [#18360](https://github.com/pnpm/pnpm/issues/18360).
