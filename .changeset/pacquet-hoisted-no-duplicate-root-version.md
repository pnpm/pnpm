---
"pacquet": patch
---

With `nodeLinker: hoisted`, a workspace project no longer gets its own copy of a dependency whose version already won the workspace-root slot. Only the versions that lost the root slot are nested, matching the pnpm CLI. Previously every project's direct dependency was materialized under that project as well, which gave lifecycle scripts a second copy to run in.
