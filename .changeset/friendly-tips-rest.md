---
"@pnpm/resolve-dependencies": patch
---

Fix a case in `resolveDependencies`, whereby an importer that should not have been updated altogether, was being updated when `updateToLatest` was specified in the options.
