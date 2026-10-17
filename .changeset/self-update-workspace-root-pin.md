---
"pacquet": patch
---

`pnpm self-update` run inside a workspace package now updates the pnpm pin in the workspace root's manifest. It also updates a pin declared in `package.yaml` or `package.json5`.
