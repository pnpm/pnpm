---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm --filter <project> update <pkg>` now fails with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` when the selected projects do not depend on `<pkg>`, also in a workspace with a shared lockfile and a root project. It used to install and exit successfully.
