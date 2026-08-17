---
"@pnpm/installing.deps-restorer": patch
"pacquet": patch
"pnpm": patch
---

Under `nodeLinker: hoisted`, `pnpm install` now clears the orphaned package directories an interrupted or failed install leaves in a project's `node_modules`. A directory the previous install recorded placing is removed; one pnpm has no record of installing is moved to `node_modules/.ignored` instead of being deleted [#13676](https://github.com/pnpm/pnpm/issues/13676).
