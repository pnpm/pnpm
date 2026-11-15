---
"@pnpm/fs.packlist": patch
"pnpm": patch
---

`pnpm pack` and `pnpm publish` in a workspace now keep the workspace's ignore rules when a package's `.npmignore` is removed while pnpm lists the package's files.
