---
"@pnpm/fetch": patch
"pnpm": patch
---

When making requests for the non-abbreviated packument, add `*/*` to the `Accept` header to avoid getting a 406 error on AWS CodeArtifact [#9862](https://github.com/pnpm/pnpm/issues/9862).
