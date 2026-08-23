---
"pacquet": patch
---

`pnpm -r update --latest --depth 0 <selector>` now fails with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` when no project in the workspace declares a matching dependency, instead of silently doing nothing.
