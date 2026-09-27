---
"pacquet": patch
---

`pnpx --version` and `pnpm dlx --version` now print the pnpm version. Other unknown options before the command are reported as errors. Before, pnpm tried to download a package named after the option [#16259](https://github.com/pnpm/pnpm/issues/16259).
