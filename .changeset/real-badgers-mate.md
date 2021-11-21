---
"@pnpm/headless": patch
"pnpm": patch
---

The postinstall scripts of dependencies should be executed after the root dependencies of the project are symlinked [#4018](https://github.com/pnpm/pnpm/issues/4018).
