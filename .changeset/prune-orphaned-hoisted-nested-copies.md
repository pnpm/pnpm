---
"@pnpm/installing.deps-restorer": patch
"pacquet": patch
"pnpm": patch
---

Under `nodeLinker: hoisted`, `pnpm install` now prunes orphaned package directories left in a project's `node_modules` by an interrupted or failed install [#13676](https://github.com/pnpm/pnpm/issues/13676).
