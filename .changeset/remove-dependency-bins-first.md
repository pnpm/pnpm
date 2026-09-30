---
"@pnpm/installing.linking.modules-cleaner": patch
"pnpm": patch
---

Removing a dependency whose bins are declared through `directories.bin` no longer leaves broken shims in `node_modules/.bin`. pnpm now removes the bins before it deletes the package directory.
