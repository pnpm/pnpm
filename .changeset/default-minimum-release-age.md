---
"@pnpm/config.reader": minor
"pnpm": minor
---

Set default `minimumReleaseAge` to 4320 minutes (3 days) to protect against supply chain attacks by default. Packages published less than 3 days ago will not be installed unless users explicitly set `minimum-release-age=0` in their `.npmrc` or `pnpm-workspace.yaml`.
