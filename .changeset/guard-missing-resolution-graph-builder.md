---
"@pnpm/deps.graph-builder": patch
"pnpm": patch
---

Fix `TypeError: Cannot use 'in' operator to search for 'directory' in undefined` during `pnpm install --frozen-lockfile` when a peer-dep variant snapshot omits its `resolution` field (the variant inherits resolution from the base entry, so this shape is valid in the lockfile but the graph builder didn't guard the access).
