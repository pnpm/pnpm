---
"@pnpm/package-requester": minor
"@pnpm/deps.graph-builder": minor
"@pnpm/headless": minor
"@pnpm/core": minor
"pnpm": minor
---

When `supportedArchitectures` specifies multiple os/cpu combinations, all matching Node.js runtime variants are now automatically installed alongside the primary one. Each extra variant is installed into `node_modules/node-<os>-<cpu>[-musl]/`.
