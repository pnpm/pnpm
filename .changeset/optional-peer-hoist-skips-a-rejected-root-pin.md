---
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

The workspace root's own dependency on a package no longer bounds an auto-installed *optional* peer when it falls outside the declared peer range. A root that pins `date-fns@2.30.0` used to push that version into an importer whose only `date-fns` need was an optional `^4.0.0` peer, so pnpm reported its own resolution as unmet even though a satisfying `date-fns@4.4.0` was already in the graph. The root's pin now bounds the candidates only when it overlaps the wanted range [#13867](https://github.com/pnpm/pnpm/issues/13867).
