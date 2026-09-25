---
"@pnpm/workspace.projects-reader": patch
"pnpm": patch
---

Workspace discovery now returns one project per directory when multiple manifest formats are present. It selects `package.json`, then `package.json5`, then `package.yaml` [pnpm/pnpm#3027](https://github.com/pnpm/pnpm/issues/3027).
