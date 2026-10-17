---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

Trusted publishing with OIDC now works with registries whose URL has a path, such as `https://registry.example/npm/private/`. pnpm previously sent the token exchange request to the root of the registry host.
