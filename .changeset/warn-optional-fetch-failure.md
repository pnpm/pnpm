---
"pacquet": patch
---

`pnpm install` now prints a warning with the error when an optional dependency cannot be fetched and is skipped. The skipped package is no longer counted in the `Packages: +N` summary. The `pnpm:skipped-optional-dependency` log reports the skip with the `fetch_failure` reason [#16514](https://github.com/pnpm/pnpm/issues/16514).
