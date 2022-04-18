---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Fix an error with peer resolutions, which was happening when there was a circular dependency and another dependency that had the name of the circular dependency as a substring.
