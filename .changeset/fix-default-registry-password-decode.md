---
"@pnpm/network.auth-header": patch
"pnpm": patch
---

Fix `_password` handling for the default registry to decode from base64 before use, consistent with scoped registry behavior.
