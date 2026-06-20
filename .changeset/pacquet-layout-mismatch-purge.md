---
"pacquet-package-manager": patch
"pnpm": patch
---

Pacquet will now selectively purge stale `node_modules` content rather than deleting the entire directory when a layout mismatch is detected, matching pnpm's behavior. Additionally, it now enforces that the module directory being deleted is safely contained within the workspace root.
