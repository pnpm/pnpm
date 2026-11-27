---
"@pnpm/pnpr": patch
---

When pnpr listens on port 0, the default public URL uses the port assigned by the OS. Wrappers can read the bound address from the JSON log event named `listening`.
