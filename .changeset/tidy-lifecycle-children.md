---
"@pnpm/exec.lifecycle": patch
"pnpm": patch
---

Fixed recursive `run` cleanup on Windows when a lifecycle script fails while another script's process tree is still running.
