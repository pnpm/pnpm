---
"pacquet": patch
---

pnpm now passes an interrupt on to the handler it replaced, with the arguments that handler expects. A handler that asked for the signal's details was called without them. Only a program that embeds pnpm and installs its own `SIGINT`, `SIGTERM` or `SIGHUP` handler was affected.
