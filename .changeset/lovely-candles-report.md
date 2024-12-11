---
"@pnpm/default-resolver": major
"@pnpm/tarball-resolver": major
"@pnpm/core": major
"pnpm": major
---

Dependencies specified via a URL are now recorded in the lockfile using their final resolved URL. Thus, if the original URL redirects, the final redirect target will be saved in the lockfile [#8833](https://github.com/pnpm/pnpm/issues/8833).
