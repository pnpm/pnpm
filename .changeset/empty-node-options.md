---
"pacquet": patch
---

Empty `nodeOptions` values from command-line flags and environment variables now override lower-priority settings. Scripts retain `NODE_OPTIONS` from the parent environment or `extraEnv` when `nodeOptions` is empty.
