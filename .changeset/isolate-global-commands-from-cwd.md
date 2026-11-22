---
"@pnpm/cli.parse-cli-args": patch
"@pnpm/config.reader": patch
"@pnpm/deps.inspection.commands": patch
"pnpm": patch
---

Global commands (`--global`, `-g`) no longer load `.pnpmfile.cjs`, `pnpm-workspace.yaml`, or `.npmrc` from the current working directory. `pnpm why -g` now inspects globally installed packages.
