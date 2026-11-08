---
"@pnpm/lockfile.verification": patch
"@pnpm/resolving.local-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` now accepts a lockfile when an injected workspace package declares a `file:` or `link:` dependency with backslashes, such as `file:..\vendor`, or with a slash after the protocol, such as `file:/./vendor`. Before, the install reported the lockfile as out of date right after `pnpm install` had written it [#17720](https://github.com/pnpm/pnpm/pull/17720).
