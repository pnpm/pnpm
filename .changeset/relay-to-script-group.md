---
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
"pacquet": patch
---

A signal sent to pnpm while it runs without a terminal, as a container runtime or a service manager does, now reaches the script even when the shell running it stays the script's parent, and pnpm waits for the script to finish shutting down. `sh` implementations such as dash keep running as the parent of a script like `node server.js`, and a signal relayed to the shell alone used to end the shell at once or stay with it, while the script was never told to stop.
