---
"pacquet": patch
---

pnpm now checks the expected hash before reusing a tarball that another resolution downloaded earlier in the same install. Two resolutions that name one URL but pin different integrities no longer share one download [#15021](https://github.com/pnpm/pnpm/issues/15021).
