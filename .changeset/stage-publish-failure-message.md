---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

When the registry rejects `pnpm stage publish`, the error message now starts with "Failed to stage package".
