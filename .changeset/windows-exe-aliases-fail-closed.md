---
"@pnpm/exe": patch
"pnpm": patch
---

On Windows, `pn`, `pnpx`, and `pnx` from `@pnpm/exe` now run only the adjacent native binary. If the package's install script was blocked, an unrelated `pnpm` from `PATH` could run. The wrappers now report clear reinstall guidance [#14885](https://github.com/pnpm/pnpm/issues/14885).
