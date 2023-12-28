---
"@pnpm/npm-resolver": patch
"pnpm": patch
---

Running `pnpm update -r --latest` and `pnpm install pkg@latest` will no longer downgrade prerelease dependencies [#7436](https://github.com/pnpm/pnpm/issues/7436).
