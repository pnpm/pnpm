---
"@pnpm/pnpr": minor
---

pnpr now records whether a package version has lifecycle scripts or `binding.gyp` when it reads the version's tar. It serves that boolean as `dist.pnpr.requiresBuild` in full packuments, abbreviated packuments, and single-version manifests on `.tgz` and tar registries alike, and as `requiresBuild` on `POST /-/pnpr/v0/resolve` `package` frames. pnpr strips any `dist.pnpr` field sent by a publisher or an upstream registry.
