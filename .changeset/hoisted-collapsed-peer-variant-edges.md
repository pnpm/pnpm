---
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.linking.real-hoist": patch
"pacquet": patch
"pnpm": patch
---

Under `nodeLinker: hoisted`, a dependency declared against a peer-resolution variant of a package version is no longer dropped from the installed layout. All variants of a version share one hoisted copy, and edges pointing at any of them now resolve to it, so the depending project keeps the package in its `.package-map.json` and the depending package keeps it in its `node_modules/.bin`.
