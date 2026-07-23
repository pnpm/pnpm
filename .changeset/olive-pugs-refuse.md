---
"@pnpm/config.parse-overrides": minor
"@pnpm/hooks.read-package-hook": patch
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

`optimisticRepeatInstall` now respects `overrides`: a local file dependency (`file:` or a bare local path) that is replaced by an override no longer disables the repeat-install fast path, since the override's target is what actually gets installed [#12892](https://github.com/pnpm/pnpm/issues/12892).
