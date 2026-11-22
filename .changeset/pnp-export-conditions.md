---
"@pnpm/lockfile.to-pnp": patch
"pnpm": patch
---

Plug'n'Play now combines custom export conditions from `NODE_OPTIONS` and command-line flags. It also respects Node.js's enabled `node-addons` and `module-sync` conditions.
