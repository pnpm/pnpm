---
"pacquet": patch
---

pnpm no longer replaces the pnpm version recorded in `pnpm-lock.yaml` for a `devEngines.packageManager` range with the running pnpm while the recorded version still satisfies the range [#16431](https://github.com/pnpm/pnpm/issues/16431).
