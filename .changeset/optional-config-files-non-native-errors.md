---
"@pnpm/workspace.workspace-manifest-reader": patch
"@pnpm/workspace.workspace-manifest-writer": patch
"@pnpm/config.commands": patch
"@pnpm/auth.commands": patch
"pnpm": patch
---

pnpm now treats a missing global `config.yaml`, `auth.ini`, or other optional config file as absent in Node.js-compatible runtimes such as StackBlitz WebContainers. Commands such as `pnpm --version` failed there with `ENOENT` [#14030](https://github.com/pnpm/pnpm/issues/14030).
