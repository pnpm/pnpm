---
"pacquet": patch
---

On Windows, `pnpm self-update` no longer runs the update a second time when it replaces a `pnpm.cmd` linked by pnpm 12.8 or older. cmd.exe read on in the replaced `pnpm.cmd`, printed an error about a command that is not recognized, and ran the new pnpm once more [#16573](https://github.com/pnpm/pnpm/issues/16573).
