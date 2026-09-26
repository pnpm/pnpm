---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update <pkg>` now moves a package off a locked version the registry no longer serves, such as an unpublished release. The lockfile check for supply-chain policies such as `minimumReleaseAge` used to reject that version before the update could replace it [pnpm/pnpm#9953](https://github.com/pnpm/pnpm/issues/9953).
