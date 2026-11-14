---
"@pnpm/building.commands": patch
"@pnpm/exec.commands": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm dlx` now keys its cache on active `overrides`, `packageExtensions`, `minimumReleaseAge`, `trustPolicy`, `trustLockfile`, and `blockExoticSubdeps` settings. Cached installs created under looser settings are no longer reused when stricter policies apply.

`pnpm dlx --allow-build=!<pkg>` now denies build scripts for `<pkg>`, including the executed package itself. Denied packages also skip interactive build approval [#18075](https://github.com/pnpm/pnpm/pull/18075).
