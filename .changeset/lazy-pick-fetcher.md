---
"@pnpm/installing.package-requester": patch
"pnpm": patch
---

Avoid selecting package fetchers for skip-fetch installs when a registry tarball already has integrity metadata.
