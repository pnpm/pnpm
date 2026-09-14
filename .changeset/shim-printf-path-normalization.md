---
"pacquet": patch
---

POSIX bin shims now pass the shim path through `printf` before converting backslashes to slashes. A POSIX `echo` used to turn sequences such as `\n` in a Windows-form path into a newline [#14867](https://github.com/pnpm/pnpm/issues/14867). Reinstalling replaces the shims already in your `node_modules`.
