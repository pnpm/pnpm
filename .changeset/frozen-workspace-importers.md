---
"pacquet": patch
---

Fixed `pnpm install --frozen-lockfile` installing dependencies of projects removed from `pnpm-workspace.yaml`. Missing local tarballs used only by those projects no longer fail the install [#15248](https://github.com/pnpm/pnpm/issues/15248).
