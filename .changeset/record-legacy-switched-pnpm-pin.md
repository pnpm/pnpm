---
"pnpm": patch
"pacquet": patch
---

pnpm started by pnpm 10's version switch now records the pinned pnpm version in a fresh `pnpm-lock.yaml`. A later `pnpm install --frozen-lockfile` run directly then succeeds [#17303](https://github.com/pnpm/pnpm/issues/17303).
