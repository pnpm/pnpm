---
"pacquet": patch
---

With `nodeLinker: hoisted` and `autoInstallPeers: false`, a package that workspace projects use with different peer versions is now installed once per peer variant, so each project's copy resolves that project's peer [#17395](https://github.com/pnpm/pnpm/issues/17395).
