---
"pacquet": minor
---

With `supportedArchitectures` set, pnpm fetches into the store the archives of a package with platform variants (from a registry declared `serverType: pnpr`) that its other platforms need, beside the one it links. That is every variant that matches one of the listed platforms, plus the version's own archive for a platform that no variant can be linked on.
