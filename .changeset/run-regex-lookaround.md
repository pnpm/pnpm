---
"pacquet": patch
---

`pnpm run "/<regex>/"` now accepts JavaScript regular expression syntax such as lookahead and lookbehind. A selector like `"/^hello:(?!b).*$/"` failed with `ERR_PNPM_NO_SCRIPT` [#16604](https://github.com/pnpm/pnpm/issues/16604).
