---
"@pnpm/tarball-resolver": patch
"pnpm": patch
---

When a dependency is installed via a direct URL that redirects to another URL and is immutable, the original URL is normalized and saved to `package.json` [#10197](https://github.com/pnpm/pnpm/pull/10197).
