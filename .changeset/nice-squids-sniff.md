---
"@pnpm/plugin-commands-rebuild": patch
---

The pnpm rebuild command should not add pkgs included in `ignoredBuiltDependencies` to `ignoredBuilds`.
