---
"@pnpm/core": major
"pnpm": major
---

Direct dependencies are deduped. So if the same dependency is both in a project and in the workspace root, then it is only linked to the workspace root.
