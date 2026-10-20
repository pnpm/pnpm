---
"@pnpm/types": patch
"@pnpm/config.reader": patch
"@pnpm/config.commands": patch
"@pnpm/installing.linking.real-hoist": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

pnpm now recognizes `externalDependencies` and `autoInstallPeersFromHighestMatch` in `pnpm-workspace.yaml` without warning and forwards both settings to `pnpm install`. Previously, setting `externalDependencies` in `pnpm-workspace.yaml` with `nodeLinker: hoisted` crashed `pnpm install` with `externalDependencies.has is not a function` [pnpm/tasks#166](https://github.com/pnpm/tasks/issues/166).
