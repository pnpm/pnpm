---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"@pnpm/engine.pm.commands": patch
"@pnpm/exe": patch
"pnpm": patch
"pacquet": patch
---

On Nix, a dependency's bin named like a system utility such as `sed` can no longer redirect a POSIX bin shim or the `pnpm`, `pn`, `pnpx`, and `pnx` launchers. The shims and launchers now ignore `node_modules` and relative `PATH` entries while they locate their own files. Installing again replaces the shims already in `node_modules` [#14883](https://github.com/pnpm/pnpm/issues/14883).
