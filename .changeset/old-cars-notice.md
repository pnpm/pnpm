---
"@pnpm/dependency-path": major
"@pnpm/plugin-commands-installation": minor
"@pnpm/plugin-commands-publishing": minor
"@pnpm/plugin-commands-script-runners": minor
"@pnpm/plugin-commands-licenses": minor
"@pnpm/plugin-commands-outdated": minor
"@pnpm/plugin-commands-patching": minor
"@pnpm/read-projects-context": minor
"@pnpm/plugin-commands-listing": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/plugin-commands-deploy": minor
"@pnpm/reviewing.dependencies-hierarchy": minor
"@pnpm/plugin-commands-audit": minor
"@pnpm/store-connection-manager": minor
"@pnpm/package-requester": minor
"@pnpm/plugin-commands-rebuild": minor
"@pnpm/modules-cleaner": minor
"@pnpm/plugin-commands-store": minor
"@pnpm/license-scanner": minor
"@pnpm/lockfile-to-pnp": minor
"@pnpm/modules-yaml": minor
"@pnpm/lockfile-utils": minor
"@pnpm/get-context": minor
"@pnpm/mount-modules": minor
"@pnpm/headless": minor
"@pnpm/package-store": minor
"@pnpm/deps.graph-builder": minor
"@pnpm/hoist": minor
"@pnpm/core": minor
"@pnpm/audit": minor
"@pnpm/list": minor
"@pnpm/config": minor
"@pnpm/server": minor
---

New setting called `virtual-store-dir-max-length` added for modifying the max allowed length of the directories inside `node_modules/.pnpm`. The default length is 120 characters [#7355](https://github.com/pnpm/pnpm/issues/7355).
