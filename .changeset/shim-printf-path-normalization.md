---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

POSIX bin shims now convert a Windows-form path such as `C:\node_modules\.bin\tsc` without corrupting it [#14867](https://github.com/pnpm/pnpm/issues/14867). The shim turned the `\n` of `\node_modules` into a newline and the `\t` of `\tsc` into a tab, so it could not find the package it runs. Installing again replaces the shims already in `node_modules`.
