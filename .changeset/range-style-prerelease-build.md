---
"pacquet": patch
---

`pnpm add` and `pnpm update` now keep the range operator of a specifier that has both a prerelease and a build suffix, such as `^1.0.0-next.0+build.1`.
