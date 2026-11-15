---
"@pnpm/engine.runtime.commands": patch
"@pnpm/engine.runtime.node-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm env` commands (`use`, `list`, and `remove`) now accept Node.js version specifiers with a leading `v`, including prerelease versions such as `v22.0.0-rc.1`.
