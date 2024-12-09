---
"@pnpm/plugin-commands-deploy": minor
"pnpm": minor
---

`pnpm deploy` now tries creating a dedicated lockfile from a shared lockfile for deployment. It will fallback to deployment without a lockfile if there's not enough condition to create one.
