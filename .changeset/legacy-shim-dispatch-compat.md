---
"pacquet": patch
---

After a self-update from pnpm 12.2 to 12.3, global commands such as `node`, `npm`, and `yarn` failed with `unexpected argument '--shim' found`. Global commands now launch normally, and their first launch migrates the global bin directory to native shims.
