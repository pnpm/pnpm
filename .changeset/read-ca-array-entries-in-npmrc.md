---
"pnpm": patch
---

pnpm now trusts the certificate authorities listed with `ca[]=` in `.npmrc`, the array form the `ca` setting documents. They were ignored, so requests to a registry with a private certificate authority failed with `invalid peer certificate: UnknownIssuer`.
