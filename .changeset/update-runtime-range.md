---
"pacquet": patch
---

`pnpm update` now moves the `devEngines.runtime` and `engines.runtime` version ranges onto the Node.js version the run resolves, the same way it moves an ordinary dependency's range. The manifest kept its old range while the lockfile already recorded the new runtime version [#14988](https://github.com/pnpm/pnpm/issues/14988).
