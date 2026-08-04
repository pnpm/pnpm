---
"pacquet": minor
---

Command shims now set `NODE_PATH` the way pnpm does: under the isolated `nodeLinker` with a hoist pattern, each shim lists the target package's own `node_modules` directories followed by the hidden hoisted modules directory (`node_modules/.pnpm/node_modules`). The new `extendNodePath: false` setting turns this off.
