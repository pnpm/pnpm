---
"@pnpm/config.reader": major
"pnpm": major
---

Removed the `ignore-dep-scripts` setting. It is no longer needed because dependency build scripts are already blocked by default — use `allowBuilds` in `pnpm-workspace.yaml` to allow specific packages to run scripts.
