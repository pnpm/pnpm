---
"@pnpm/node.resolver": patch
"pnpm": patch
---

Include musl Linux variants when resolving `node@runtime:` dependencies. The lockfile now includes musl builds (from `unofficial-builds.nodejs.org`) alongside the standard glibc variants, so that `node@runtime:` works out of the box on Alpine Linux and other musl-based distributions.
