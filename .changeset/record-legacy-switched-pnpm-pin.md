---
"pacquet": patch
---

Fresh lockfiles now include the pinned pnpm version when an older pnpm starts it. Subsequent `pnpm install --frozen-lockfile` commands can use these lockfiles [#17303](https://github.com/pnpm/pnpm/issues/17303).
