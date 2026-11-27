---
"pnpm": patch
---

Fixed installs failing in shared Unix projects when another user owns a package executable that the current user can already run [#18491](https://github.com/pnpm/pnpm/issues/18491).
