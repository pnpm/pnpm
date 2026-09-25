---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

Secondary dependencies now prefer the version resolved by the local project's direct dependencies over versions from sibling workspace projects [pnpm/pnpm#7191](https://github.com/pnpm/pnpm/issues/7191).
