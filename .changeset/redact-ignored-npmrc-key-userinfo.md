---
"@pnpm/config.reader": patch
"pnpm": patch
---

The warnings about ignored project `.npmrc` registry and auth settings no longer print the username and password of a URL-scoped key such as `//user:password@registry.example.com/:_authToken`.
