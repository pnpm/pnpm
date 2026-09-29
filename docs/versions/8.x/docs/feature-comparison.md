---
id: feature-comparison
title: Feature Comparison
---

| Feature | pnpm | Yarn | npm |
| ---     | ---  | ---  | --- |
| Workspace support | ✔️ | ✔️ | ✔️ |
| Isolated `node_modules` | ✔️ - The default | ✔️ | ✔️ |
| Hoisted `node_modules` | ✔️ | ✔️ | ✔️ - The default |
| Autoinstalling peers | ✔️ | ❌ | ✔️  |
| Plug'n'Play | ✔️ | ✔️ - The default | ❌ |
| Zero-Installs | ❌ | ✔️ | ❌ |
| Patching dependencies | ✔️ | ✔️ | ❌ |
| Managing Node.js versions | ✔️ | ❌ | ❌ |
| Has a lockfile | ✔️ - `pnpm-lock.yaml` | ✔️ - `yarn.lock` | ✔️ - `package-lock.json` |
| Overrides support | ✔️ | ✔️ - Via resolutions | ✔️ |
| Content-addressable storage | ✔️ | ❌ | ❌ |
| Dynamic package execution | ✔️ - Via `pnpm dlx` | ✔️ - Via `yarn dlx` | ✔️ - Via `npx` |
| Side-effects cache | ✔️ | ❌ | ❌ |
| Listing licenses | ✔️  - Via `pnpm licenses list` | ✔️  - Via a plugin | ❌ |
