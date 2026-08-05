---
"pacquet": minor
---

`sharedWorkspaceLockfile: false` is now supported by the install family [#12042](https://github.com/pnpm/pnpm/issues/12042): a workspace install runs one dedicated install per project, each with its own `pnpm-lock.yaml`, `node_modules`, and virtual store (a custom `virtualStoreDir` resolves per project), and `pnpm add` / `update` / `remove` in a project operate on that project's own lockfile. Recursive and filtered install-family commands still require a shared lockfile.
