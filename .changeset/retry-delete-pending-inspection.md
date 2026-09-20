---
"@pnpm/fs.graceful-fs": patch
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
"pacquet": patch
---

Fixed installs in different projects failing on Windows when they share a global virtual store, with `Access is denied` while inspecting a file in the slot they were both repairing [#15114](https://github.com/pnpm/pnpm/issues/15114). Windows reports a file another process has just deleted as inaccessible until the deletion finishes, and the install now waits for that instead of stopping.
