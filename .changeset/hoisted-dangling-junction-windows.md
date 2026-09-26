---
"pacquet": patch
---

On Windows, `pnpm install` with `nodeLinker: hoisted` no longer fails with "Access is denied" after one workspace project's `node_modules` was deleted. Another project's copy of a shared dependency could link to the deleted directory, and pnpm now replaces that broken link with a real directory.
