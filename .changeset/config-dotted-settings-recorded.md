---
"pacquet": patch
---

`pnpm config get` and `pnpm config list` now report a setting given on the command line with `--config.<name>=<value>`. Before, a value such as `--config.node-linker=hoisted` reached the install but was absent from the reported configuration [#16276](https://github.com/pnpm/pnpm/issues/16276).
