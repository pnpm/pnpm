---
"@pnpm/config": minor
"pnpm": minor
---

Support specifying the pnpm version via `devEngines.packageManager` in `package.json`. Unlike the `packageManager` field, this supports version ranges. The resolved version is stored in `pnpm-lock.yaml` and reused if it still satisfies the range.
