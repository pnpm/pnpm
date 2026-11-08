---
"pacquet": patch
---

With `nodeLinker: hoisted`, pnpm no longer deletes a directory outside `node_modules` when it removes a package whose scope directory is a symlink.
