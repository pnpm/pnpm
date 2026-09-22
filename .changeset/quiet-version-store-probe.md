---
"pacquet": patch
---

Fixed `pnpm --version` creating a temporary file in the project directory during store detection. This prevents file watchers such as Nx from repeatedly rebuilding their project graph.
