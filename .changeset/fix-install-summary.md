---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Fixed the install summary in workspaces by setting recursive config before reporter initialization and skipping manifest logging during dry runs.
