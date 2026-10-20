---
"@pnpm/pnpr": minor
---

pnpr now serves `dist.pnpr.requiresBuild` in package metadata once it has scanned a version's tarball for build triggers. `POST /-/pnpr/v0/resolve` also includes `requiresBuild` on `package` frames. pnpr strips any `dist.pnpr` field sent by a publisher or an upstream registry.
