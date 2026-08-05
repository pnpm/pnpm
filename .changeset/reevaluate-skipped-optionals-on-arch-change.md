---
"pacquet": patch
---

Changing `--os` / `--cpu` / `--libc` or `supportedArchitectures` between installs now re-evaluates previously skipped optional dependencies, so the platform packages for the newly selected architecture are installed instead of staying skipped.
