---
"pacquet": patch
---

Sped up `pnpm install` on Macs that write files fastest with few threads. On macOS, pnpm now measures how many files the volume writes fastest at once while it links `node_modules`, and keeps that many in flight. pnpm saves the measured width in the store and starts the next install into the same volume from it. Set `PNPM_MACOS_LINK_CONCURRENCY` to a positive number to use a fixed width instead.
