---
"@pnpm/deps.inspection.peers-checker": minor
"@pnpm/deps.inspection.commands": patch
"pnpm": patch
---

Fix incorrect missing peer dependency warnings in monorepos. When checking peer dependencies via the lockfile (used by `pnpm peers` and friends), peers that are provided by the workspace root importer are no longer reported as missing for sub-projects. This matches the install-time behavior governed by `resolvePeersFromWorkspaceRoot`. The new behavior is enabled by default and can be disabled by setting `resolvePeersFromWorkspaceRoot: false`. See pnpm/pnpm#1284.
