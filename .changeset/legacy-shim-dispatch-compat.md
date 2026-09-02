---
"pacquet": patch
---

Global commands such as `node`, `npm`, and `yarn` no longer fail with `unexpected argument '--shim' found` after a self-update from pnpm 12.2 to 12.3. The first launch of such a command now migrates the global bin directory to the native shims that pnpm 12.3 writes.
