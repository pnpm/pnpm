---
"pacquet": patch
---

Reduced per-package filesystem syscalls when materializing the virtual store on macOS: each package slot is now created with direct `mkdir` calls instead of a recursive probe, and unscoped packages skip a redundant parent-directory check in the directory-clone cache, making warm installs that rebuild `node_modules` about 10% faster.
