---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

When `dedupe-peer-dependents` is enabled (default), use the path (not id) to
determine compatibility.

When multiple dependency groups can be deduplicated, the
latter ones are sorted according to number of peers to allow them to
benefit from deduplication.

Resolves: [#6605](https://github.com/pnpm/pnpm/issues/6605)
