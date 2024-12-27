---
"@pnpm/git-resolver": patch
"pnpm": patch
---

Do not fall back to SSH, when resolving a git-hosted package if `git ls-remote` works via HTTPS [#8906](https://github.com/pnpm/pnpm/pull/8906).
