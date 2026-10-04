---
"pacquet": patch
---

On Windows, `pnpm self-update` from pnpm 12.9.0 or older no longer runs the update a second time. cmd.exe read on in the replaced `pnpm.cmd`, printed an error about a command that is not recognized, and ran the new pnpm once more [#16573](https://github.com/pnpm/pnpm/issues/16573).
