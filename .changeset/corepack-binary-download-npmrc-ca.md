---
"pacquet": patch
---

When the `pnpm` package downloads its native binary on first run, as it does under Corepack, it now trusts the CA certificates from the `ca`, `ca[]`, and `cafile` settings of `.npmrc`, including the ones scoped to the registry. A registry behind a private CA failed with `fetch failed` before, unless `NODE_EXTRA_CA_CERTS` named the CA. A project `.npmrc` is used only while the download is checked against npm's signature. Node.js versions that cannot add CA certificates after startup still need `NODE_EXTRA_CA_CERTS`.
