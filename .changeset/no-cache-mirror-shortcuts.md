---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Resolving against a registry that sends `Cache-Control: no-cache`, such as Azure Artifacts, is fast again when the lockfile already decides the version. pnpm reads the cached metadata for a pinned version, or for a range the lockfile version is proven to win, without a registry request [#17758](https://github.com/pnpm/pnpm/issues/17758).
