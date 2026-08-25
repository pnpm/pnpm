---
"pacquet": patch
---

Dependencies declared with an empty version range (`"adler-32": ""`) install again instead of failing with `ERR_PNPM_NO_MATCHING_VERSION` [#13673](https://github.com/pnpm/pnpm/issues/13673). An omitted range means "any version", as it does in npm and pnpm v11, so packages that publish one — such as `js-xlsx`, `codepage`, and `ssf` — no longer need an `overrides` entry to install.
