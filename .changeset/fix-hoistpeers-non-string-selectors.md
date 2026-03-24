---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Fixed handling of non-string version selectors in `hoistPeers`, preventing invalid peer dependency specifiers.
