---
"@pnpm/deps.inspection.peers-issues-renderer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm peers check` and the `ERR_PNPM_PEER_DEP_ISSUES` error now group peer dependency issues under the workspace project they were found in [#15351](https://github.com/pnpm/pnpm/issues/15351).
