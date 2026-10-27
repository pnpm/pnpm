---
"@pnpm/package-requester": patch
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Preserve the `integrity` field of remote and git-hosted tarball dependencies in `pnpm-lock.yaml` when updating or re-resolving with a warm store [#14351](https://github.com/pnpm/pnpm/issues/14351).
