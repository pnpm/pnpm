---
"pnpm": minor
---

`pnpm install` warns when a locally declared package permission matches no dependency during a full resolution. The permission stays saved for future installs. Set `warnUnusedPermissions: false` to turn off the warning [#18429](https://github.com/pnpm/pnpm/issues/18429).
