---
"pacquet": patch
---

A repeat `pnpm install` no longer imports patched packages and packages with build scripts again when nothing changed, and no longer reruns their builds. This happened when `recursiveInstall` was `false` or the project had a `file:` dependency [#16705](https://github.com/pnpm/pnpm/issues/16705).
