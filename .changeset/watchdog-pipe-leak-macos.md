---
"pacquet": patch
---

On macOS, killing pnpm while it runs scripts in several projects at once now ends every one of those scripts. Before, a script could keep running after pnpm died.
