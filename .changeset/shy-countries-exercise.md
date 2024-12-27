---
"@pnpm/config": minor
"@pnpm/plugin-commands-deploy": minor
"pnpm": minor
---

`pnpm deploy` now tries creating a dedicated lockfile from a shared lockfile for deployment. It will fallback to deployment without a lockfile if there is no shared lockfile or `force-legacy-deploy` is set to `true`.
