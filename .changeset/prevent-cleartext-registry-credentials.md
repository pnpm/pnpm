---
"@pnpm/network.auth-header": patch
"@pnpm/config.registry-auth-key": patch
"pnpm": patch
"pacquet": patch
---

Prevent sending registry credentials over cleartext HTTP when a repository downgrades the registry URL to HTTP.
