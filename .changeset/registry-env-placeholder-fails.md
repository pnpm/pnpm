---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

A `${VAR}` placeholder in a scoped or named registry URL in `pnpm-workspace.yaml` or the project `.npmrc` now fails the command. pnpm does not expand environment variables in these files. Before, pnpm ignored such a URL and resolved the scope's packages from the default registry. A placeholder in the default `registry`, a proxy, or `userAgent` in `pnpm-workspace.yaml` is still ignored, and pnpm now prints a warning about it [#17762](https://github.com/pnpm/pnpm/issues/17762).
