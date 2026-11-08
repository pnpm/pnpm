---
"pacquet": patch
"pnpm": patch
"@pnpm/workspace.state": patch
---

Changing `runtimeOnFail` now takes effect on the next `pnpm install`. Switching to `download` installs the declared runtime even when the previous install was already up to date.
