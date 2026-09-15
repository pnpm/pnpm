---
"@pnpm/engine.pm.commands": patch
"@pnpm/exe": patch
"pnpm": patch
"pacquet": patch
---

`pn`, `pnpx`, and `pnx` now run the pnpm installed alongside them. They used to look pnpm up on `PATH`. That failed when the directory holding them was not on `PATH`, and it silently handed the call to an unrelated pnpm when one came first there [#14803](https://github.com/pnpm/pnpm/issues/14803).
