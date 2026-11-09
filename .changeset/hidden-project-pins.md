---
"get-pnpm": patch
---

`get-pnpm` now installs the version that a project in a hidden directory pins when the workspace's `packages` patterns exclude that project. It used to install the workspace root's pin for that project.
