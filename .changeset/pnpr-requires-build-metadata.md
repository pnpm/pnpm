---
"@pnpm/pnpr": minor
---

pnpr now serves `dist.pnpr.requiresBuild` in package metadata and `requiresBuild` on `POST /-/pnpr/v0/resolve` `package` frames once it has scanned a version's tarball for build triggers. pnpr strips any `dist.pnpr` field sent by a publisher or an upstream registry.
