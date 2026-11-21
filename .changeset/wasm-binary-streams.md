---
"pacquet": patch
---

pnpm in StackBlitz WebContainers now passes HTTP response bodies and subprocess output to its WebAssembly engine as raw bytes. A 64 KiB chunk no longer becomes a 234 KB JSON message [pnpm/tasks#63](https://github.com/pnpm/tasks/issues/63).
