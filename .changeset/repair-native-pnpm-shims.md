---
"pacquet": patch
---

The pnpm npm wrapper keeps its placeholder shebang-less so pnpm 11 can install pnpm 12 through the version store. Wrapper installs must allow lifecycle scripts to install the native binary [#14502](https://github.com/pnpm/pnpm/issues/14502).
