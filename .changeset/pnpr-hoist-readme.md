---
"@pnpm/pnpr": patch
---

On publish, the README of the `latest` version is now hoisted to the packument's top-level `readme` (and `readmeFilename`) field, matching npm and verdaccio. Publish clients only send the readme inside the version manifest, so without this a package published to pnpr exposed no top-level readme for full-packument consumers and registry UIs to render.
