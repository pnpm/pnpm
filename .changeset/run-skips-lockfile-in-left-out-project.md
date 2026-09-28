---
"@pnpm/exec.commands": patch
"@pnpm/workspace.root-finder": patch
"pnpm": patch
"pacquet": patch
---

`pnpm run` and `pnpm exec` in a project with no dependencies that the workspace `packages` patterns leave out run the command and do not write `node_modules` or `pnpm-lock.yaml` there [pnpm/pnpm#16313](https://github.com/pnpm/pnpm/issues/16313).
