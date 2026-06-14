---
"@pnpm/auth.commands": patch
"@pnpm/config.reader": patch
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/fetching.types": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/network.auth-header": patch
"@pnpm/pnpr.client": patch
"@pnpm/releasing.commands": patch
"@pnpm/resolving.default-resolver": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/types": patch
"pnpm": patch
---

pnpm can now use different auth tokens for different package scopes, even when those scopes use the same registry URL.

Previously, auth was selected only by registry URL. If `@org-a` and `@org-b` both used `https://npm.pkg.github.com/`, they had to share the same token. This caused problems for registries that issue tokens per organization or per scope.

Configure a scope-specific token by adding the package scope after the registry URL in the auth key:

```ini
@org-a:registry=https://npm.pkg.github.com/
@org-b:registry=https://npm.pkg.github.com/

//npm.pkg.github.com/@org-a:_authToken=${ORG_A_TOKEN}
//npm.pkg.github.com/@org-b:_authToken=${ORG_B_TOKEN}

//npm.pkg.github.com/:_authToken=${FALLBACK_TOKEN}
```

`pnpm login --registry=https://npm.pkg.github.com --scope=@org-a` writes the token to the same scope-specific auth key.

When installing or publishing `@org-a/*`, pnpm uses `ORG_A_TOKEN`. For `@org-b/*`, pnpm uses `ORG_B_TOKEN`. Packages without a matching scope continue to use the registry-wide fallback token.
