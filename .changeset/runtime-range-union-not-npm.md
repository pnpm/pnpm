---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

A `runtime:` version range that contains `||` or a space, such as a `devEngines.runtime` version of `^22.18.0 || ^24.0.0`, now installs the requested runtime. pnpm used to install the npm package with the same name, such as `node` [#14817](https://github.com/pnpm/pnpm/issues/14817).
