---
"pacquet": patch
---

With `enableGlobalVirtualStore`, an install into a fresh `node_modules` no longer runs the build scripts of a dependency whose global virtual store slot an earlier install already built. `pnpm rebuild` still runs them [#14480](https://github.com/pnpm/pnpm/issues/14480).
