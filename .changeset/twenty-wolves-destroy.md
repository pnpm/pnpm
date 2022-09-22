---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Don't crash when auto-install-peers is true and the project has many complex circular dependencies.
