---
"@pnpm/deps.status": patch
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm run` and `pnpm exec` run the requested script when the project is already installed, even if `node_modules/.pnpm-workspace-state-v1.json` is missing or unreadable. They no longer start an install first, so the script does not need a writable store or a network connection [#15173](https://github.com/pnpm/pnpm/issues/15173).
