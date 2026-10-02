---
"@pnpm/network.fetch": patch
"pnpm": patch
"pacquet": patch
---

`pnpm login` no longer forwards credentials in its request body to another origin during redirects.
