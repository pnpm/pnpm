---
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm run` and `pnpm exec` no longer install a project that has never been installed and has nothing to install. Such a project declares no dependencies, no peer dependencies that `autoInstallPeers` would fetch, and no install lifecycle scripts. The command now runs without writing `node_modules` or `pnpm-lock.yaml` [pnpm/pnpm#16313](https://github.com/pnpm/pnpm/issues/16313).
