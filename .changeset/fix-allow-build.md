---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

Fixed a bug where \`pnpm add --allow-build\` replaces existing \`allowBuilds\` config in \`pnpm-workspace.yaml\` instead of modifying it [#13872](https://github.com/pnpm/pnpm/issues/13872).
