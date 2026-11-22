---
"pacquet": patch
"pnpm": patch
"@pnpm/registry-access.commands": patch
---

When `pnpm unpublish` removes the version tagged `latest`, it prefers the highest remaining stable version.
