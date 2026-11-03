---
"pacquet": minor
---

`pnpm publish -r --batch` now uploads each package archive on its own request when the registry is declared with `serverType: pnpr`. It skips archives the registry already holds. The batch that follows names the archives by integrity, and its size no longer grows with the archives it publishes. Other registries, and batches of more than 256 packages, still carry every archive inline. [pnpm/tasks#244](https://github.com/pnpm/tasks/issues/244)
