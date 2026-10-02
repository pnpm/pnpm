---
"@pnpm/network.fetch": patch
"@pnpm/registry-access.client": patch
"pnpm": patch
"pacquet": patch
---

`pnpm login` no longer forwards credentials in its request body to another origin during redirects.
