## 11.28.8

### Patch Changes

- The standalone `@pnpm/exe` builds now embed Node.js 26.12.1. The previously embedded Node.js 26.0.0 still trusted the certificate authority intermediates that Node.js distrusted in October 2026, so a standalone pnpm accepted TLS certificates issued under them.
