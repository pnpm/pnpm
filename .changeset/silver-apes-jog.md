---
"@pnpm/build-modules": patch
"@pnpm/config": patch
"@pnpm/core": patch
"@pnpm/default-reporter": patch
"@pnpm/directory-fetcher": patch
"@pnpm/exportable-manifest": patch
"@pnpm/filter-lockfile": patch
"@pnpm/filter-workspace-packages": patch
"@pnpm/get-context": patch
"@pnpm/headless": patch
"@pnpm/hoist": patch
"@pnpm/link-bins": patch
"@pnpm/list": patch
"@pnpm/lockfile-file": patch
"@pnpm/lockfile-to-pnp": patch
"@pnpm/lockfile-utils": patch
"@pnpm/lockfile-walker": patch
"@pnpm/make-dedicated-lockfile": patch
"@pnpm/merge-lockfile-changes": patch
"@pnpm/modules-cleaner": patch
"@pnpm/outdated": patch
"@pnpm/package-requester": patch
"@pnpm/package-store": patch
"pkgs-graph": patch
"@pnpm/plugin-commands-audit": patch
"@pnpm/plugin-commands-installation": patch
"@pnpm/plugin-commands-listing": patch
"@pnpm/plugin-commands-outdated": patch
"@pnpm/plugin-commands-patching": patch
"@pnpm/plugin-commands-publishing": patch
"@pnpm/plugin-commands-rebuild": patch
"@pnpm/plugin-commands-script-runners": patch
"@pnpm/plugin-commands-server": patch
"@pnpm/plugin-commands-store": patch
"pnpm": patch
"@pnpm/prune-lockfile": patch
"@pnpm/resolve-dependencies": patch
"@pnpm/tarball-fetcher": patch
---

Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
