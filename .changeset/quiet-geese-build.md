---
"@pnpm/building.policy": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/deps.status": patch
"@pnpm/workspace.state": patch
"pnpm": patch
---

Treat `allowBuilds` as an install-state input and clear previously ignored builds when they are explicitly disallowed.
