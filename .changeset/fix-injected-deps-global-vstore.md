---
"@pnpm/deps.graph-builder": patch
"@pnpm/headless": patch
"@pnpm/core": patch
---

Fixed injected local packages to work correctly with the global virtual store [#10366](https://github.com/pnpm/pnpm/pull/10366).

When using `nodeLinker: 'isolated'` with `enableGlobalVirtualStore: true`, injected workspace packages now use the correct hash-based paths from the global virtual store instead of project-relative paths.
