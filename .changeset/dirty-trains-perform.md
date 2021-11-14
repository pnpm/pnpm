---
"@pnpm/local-resolver": patch
---

Don't fail if a local linked directory is not found (unless it should be injected). This is the intended behavior of the "link:" protocol as per Yarn's docs.
