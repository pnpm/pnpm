---
"pacquet": minor
---

`pnpm install` now uses the `dist.pnpr.requiresBuild` field from pnpr package metadata and the `requiresBuild` field from pnpr resolution frames to skip redundant `package.json` reads, skip scheduling early slot imports for packages that need a build, skip the `node --version` probe when no package in the graph needs a build, and overlap remote side-effects cache lookups with cold tarball downloads.
