---
"@pnpm/bins.cmd-shim": patch
"pnpm": patch
"pacquet": patch
---

On Windows, bin shims run from Git Bash, MSYS2, or Cygwin now pass `NODE_PATH` to Node.js as Windows paths. A project installed from cmd or PowerShell gave its bins a `NODE_PATH` under the Git install directory when they ran from Git Bash. Installing again replaces the shims already in `node_modules` [#3360](https://github.com/pnpm/pnpm/issues/3360).
