---
"pnpm": patch
---

Fixed `pnpm approve-builds` reporting "no packages awaiting approval" when a build-script dependency whose approval was revoked (e.g. after `git stash` drops the `allowBuilds` from `pnpm-workspace.yaml`) is re-added. The revoked packages are now correctly recorded in `.modules.yaml` so `approve-builds` can find them. [#12221](https://github.com/pnpm/pnpm/issues/12221)
