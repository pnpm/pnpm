---
"@pnpm/config.reader": minor
"@pnpm/exec.commands": minor
"@pnpm/exec.lifecycle": minor
"@pnpm/installing.commands": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.deps-restorer": minor
"@pnpm/lockfile.to-pnp": minor
"pnpm": minor
---

Added support for generating Node.js package maps at `node_modules/.package-map.json` during isolated and hoisted installs. Added the `node-experimental-package-map` setting to inject the generated map into pnpm-managed Node.js script environments, and the `node-package-map-type` setting to choose between `standard` and `loose` package maps.
