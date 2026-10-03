---
"@pnpm/pnpr": patch
---

pnpr can now cache images from Quay. Layer downloads that Quay redirects to its CDN no longer fail with a 502 [#16548](https://github.com/pnpm/pnpm/issues/16548).
