---
"pacquet": patch
---

`pnpm publish` with provenance from GitLab CI is no longer rejected by the npm registry with a 422 error. The provenance statement now includes the GitLab CI variables in `invocation.parameters`, as npm does [#16551](https://github.com/pnpm/pnpm/issues/16551).
