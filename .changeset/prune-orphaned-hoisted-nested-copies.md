---
"@pnpm/installing.deps-restorer": patch
"pacquet": patch
"pnpm": patch
---

Under `nodeLinker: hoisted`, `pnpm install` now clears orphaned package directories that an interrupted or failed install leaves in a project's `node_modules`. A directory recorded by the previous install is removed, while an unrecorded directory is moved to `node_modules/.ignored`. A copy already in `.ignored` is never overwritten [pnpm/pnpm#13676](https://github.com/pnpm/pnpm/issues/13676).
