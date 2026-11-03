---
"pacquet": minor
---

`pnpm publish -r --batch` to a registry declared with `serverType: pnpr` uploads each package's archive on its own request first, skipping archives the registry already holds, and then sends a batch that references the archives by integrity instead of carrying them. A release is no longer limited by the size of one request body. Other registries still get every archive inline.
