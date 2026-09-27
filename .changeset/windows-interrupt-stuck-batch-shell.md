---
"pacquet": patch
---

Interrupting a script with Ctrl+C on Windows no longer leaves the terminal stuck. A script that runs through a batch shim, as `vite dev` does through `vite.CMD`, made cmd.exe wait forever on its "Terminate batch job (Y/N)?" answer, and every following keystroke went to that prompt. pnpm now ends the script's shell when it is still running one second after the interrupt [pnpm/pnpm#14860](https://github.com/pnpm/pnpm/issues/14860).
