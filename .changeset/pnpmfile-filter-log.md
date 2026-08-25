---
"pacquet": patch
---

Deprecated the pnpmfile `filterLog` hook in pnpm v12. The Rust CLI ignores it with a warning instead of adding a Node.js round trip for every log message.
