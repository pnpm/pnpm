---
"@pnpm/config.reader": major
"pnpm": major
---

Set default `minimumReleaseAge` to 1440 minutes (1 day) to protect against supply chain attacks by default. Packages published less than 1 day ago will not be installed unless users explicitly set `minimumReleaseAge: 0` in `pnpm-workspace.yaml`.
