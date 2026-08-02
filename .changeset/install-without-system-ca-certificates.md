---
"pacquet": patch
---

`pnpm install` no longer crashes on a machine whose system certificate store is empty or absent — for example a nixpkgs sandbox with `SSL_CERT_FILE` unset [#13588](https://github.com/pnpm/pnpm/issues/13588). Such a system now falls back to the Mozilla root certificates bundled into the binary, the same set Node.js ships, so both offline and online installs work again. Certificates from the system store, `NODE_EXTRA_CA_CERTS`, and the `.npmrc` `ca` / `cafile` settings keep taking precedence whenever any of them is available.
