---
"pnpm": patch
---

pnpm should read the auth token of a github-registry-hosted package, when the registry path contains the owner [#4034](https://github.com/pnpm/pnpm/issues/4034).

So this should work:

```
@owner:registry=https://npm.pkg.github.com/owner
//npm.pkg.github.com/:_authToken=<token>
```
