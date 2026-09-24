---
"@pnpm/workspace.workspace-manifest-reader": patch
"@pnpm/workspace.workspace-manifest-writer": patch
"@pnpm/config.commands": patch
"pnpm": patch
---

pnpm no longer fails with `ENOENT` when the global `config.yaml` or another optional config file is missing in Node.js-compatible runtimes such as StackBlitz WebContainers [#14030](https://github.com/pnpm/pnpm/issues/14030).
