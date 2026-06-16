---
"@pnpm/releasing.exportable-manifest": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Avoid reading `README.md` from disk when publishing if the publish manifest already provides a `readme` field. The README is now only read lazily, inside `createExportableManifest`, when it is actually needed.
