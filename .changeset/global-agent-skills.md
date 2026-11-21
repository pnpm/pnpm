---
"pacquet": patch
---

Global installs can now link approved agent skills into agent skill directories under the home directory. `pnpm approve -g` and `pnpm permissions approve -g` review pending global skill and build approvals, and global skill decisions are stored in `globalPackagesConfig.permissions` in the global `config.yaml` [pnpm/tasks#117](https://github.com/pnpm/tasks/issues/117).
