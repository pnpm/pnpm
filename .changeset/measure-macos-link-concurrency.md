---
"pacquet": patch
---

Sped up `pnpm install` on macOS. pnpm now measures, while it writes `node_modules`, how many files the volume writes fastest at once, and keeps that many in flight. Different Macs write fastest at different widths, and the measured width is saved in the store per volume for later installs. Set the `PNPM_MACOS_LINK_CONCURRENCY` environment variable to pin the width instead.
