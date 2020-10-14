---
"@pnpm/resolve-dependencies": patch
---

When a peer dependency is not resolved but is available through `require()`, don't print a warning but still consider it to be missing.
