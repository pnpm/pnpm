---
"@pnpm/exe": patch
"pnpm": patch
---

`pn`, `pnpx`, and `pnx` from `@pnpm/exe` now report a clear error when the package's install script is blocked. The Windows wrappers no longer run an unrelated `pnpm` found on `PATH` [#14885](https://github.com/pnpm/pnpm/issues/14885).
