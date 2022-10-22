---
"pnpm": patch
---

Ignore the `always-auth` setting.

pnpm will never reuse the registry auth token for requesting the package tarball, if the package tarball is hosted on a different domain.

So, for example, if your registry is at `https://company.registry.com/` but the tarballs are hosted at `https://tarballs.com/`, then you will have to configure the auth token for both domains in your `.npmrc`:

```
@my-company:registry=https://company.registry.com/
//company.registry.com/=SOME_AUTH_TOKEN
//tarballs.com/=SOME_AUTH_TOKEN
```
