---
"pacquet": patch
---

pnpm now passes an interrupt on to the handler it replaced in the form that handler expects. A host that installed its own `SIGINT`, `SIGTERM`, or `SIGHUP` handler with `SA_SIGINFO`, as a program embedding pnpm may, was called without the signal details it takes.
