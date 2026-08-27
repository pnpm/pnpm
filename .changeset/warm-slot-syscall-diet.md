---
"pacquet": patch
---

Warm installs that rebuild `node_modules` on macOS are about 10% faster: creating each package's virtual-store directory now issues fewer filesystem calls.
