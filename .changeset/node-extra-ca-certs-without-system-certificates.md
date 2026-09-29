---
"pacquet": patch
---

Fixed installs failing with `UnknownIssuer` on Linux systems without CA certificates, such as `node:24-slim`, when `NODE_EXTRA_CA_CERTS` is set. The extra certificates now extend the bundled CA roots [#16365](https://github.com/pnpm/pnpm/issues/16365).
