---
"@pnpm/exec.commands": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm dlx` now keys its cache on active `overrides`, `minimumReleaseAge`, `trustPolicy`, `trustLockfile`, and `blockExoticSubdeps` settings. Cached installs created under looser settings are no longer reused when stricter policies apply.

`pnpm dlx --allow-build=!<pkg>` now denies build scripts for `<pkg>`, including the executed package itself, and skips interactive build approval for denied packages [zkochan/pnpm-tasks#107](https://github.com/zkochan/pnpm-tasks/issues/107).
