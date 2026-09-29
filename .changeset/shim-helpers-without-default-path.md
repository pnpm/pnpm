---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"@pnpm/engine.pm.commands": patch
"@pnpm/exe": patch
"pnpm": patch
"pacquet": patch
---

POSIX bin shims and the `pnpm`, `pn`, `pnpx`, and `pnx` launchers now run inside a Nix build, where the system default path has no `sed` or `readlink`. Installing again replaces the shims already in `node_modules` [#16377](https://github.com/pnpm/pnpm/issues/16377).
