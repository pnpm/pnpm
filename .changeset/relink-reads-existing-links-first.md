---
"pacquet": patch
---

Sped up installs in large workspaces on macOS when the dependency links already exist. pnpm now keeps a link that already points at the right package without trying to create it first. Relinking the direct dependencies of 1,000 workspace projects took 45 ms, down from 116 ms [pnpm/tasks#65](https://github.com/pnpm/tasks/issues/65).
