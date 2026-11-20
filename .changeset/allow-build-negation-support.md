---
"@pnpm/building.commands": patch
"@pnpm/building.policy": minor
"@pnpm/global.commands": major
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

Build scripts can now be rejected before the package is installed [#14067](https://github.com/pnpm/pnpm/issues/14067):

- `pnpm add --allow-build=!<pkg>` records `<pkg>: false` in `allowBuilds`. It used to write a `!<pkg>: true` entry that matched no package. On a global install the denial was dropped altogether, so the post-install prompt offered the build for approval.
- `pnpm approve-builds <pkg>` and `pnpm approve-builds !<pkg>` record their decision even when no packages are awaiting approval, and report a package that is not awaiting approval with a warning so a typo stays visible. Both cases used to fail the command with an error.
