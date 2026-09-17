---
"pacquet": patch
---

pnpm no longer reuses one package's downloaded tarball for another package whose resolution pins a different integrity hash to the same URL [#15021](https://github.com/pnpm/pnpm/issues/15021).
