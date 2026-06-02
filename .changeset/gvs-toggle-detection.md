---
"@pnpm/workspace.state": patch
"@pnpm/deps.status": patch
"pnpm": patch
---

Fix `pnpm install` ignoring `enableGlobalVirtualStore` toggle by including it in the workspace state settings check [#12142](https://github.com/pnpm/pnpm/issues/12142).
