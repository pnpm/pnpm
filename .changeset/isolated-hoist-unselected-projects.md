---
"pacquet": patch
---

A filtered first install under the isolated linker now links every workspace project by name into `node_modules/.pnpm/node_modules`, not only the selected ones. The hoist plan was built from the selected projects' manifests, so `hoistWorkspacePackages` shrank to the filter and the unselected projects vanished from the private hoisted modules dir [pnpm/tasks#155](https://github.com/pnpm/tasks/issues/155).
