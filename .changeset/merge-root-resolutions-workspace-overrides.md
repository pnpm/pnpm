---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

Fixed workspace installs so root `package.json#resolutions` are merged with `pnpm-workspace.yaml#overrides` instead of replacing workspace overrides during lockfile generation.
