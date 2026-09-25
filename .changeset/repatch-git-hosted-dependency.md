---
"@pnpm/patching.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm patch` now applies the existing patch file to the edit directory of a git-hosted dependency, as it already does for packages from the registry [#9699](https://github.com/pnpm/pnpm/issues/9699).
