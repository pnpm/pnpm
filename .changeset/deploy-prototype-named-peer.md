---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm deploy` in a workspace with `injectWorkspacePackages` no longer crashes when a linked workspace package declares a peer dependency named after an `Object.prototype` member, such as `constructor`.
