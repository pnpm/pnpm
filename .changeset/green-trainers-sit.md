---
"@pnpm/local-resolver": major
"@pnpm/package-requester": major
"pnpm": major
---

Local dependencies referenced through the `file:` protocol are hard linked (not symlinked) [#4408](https://github.com/pnpm/pnpm/pull/4408). If you need to symlink a dependency, use the `link:` protocol instead.
