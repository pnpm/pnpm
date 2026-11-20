---
"@pnpm/pnpr": minor
---

Packument responses now carry a `Last-Modified` header derived from the document's `time.modified`, so a client's release-age check can learn the package-level last-publish bound from a cheap `HEAD` probe instead of downloading the metadata body.
