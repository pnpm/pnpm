---
"pacquet": patch
---

Concurrent installs sharing a global virtual store serialize forced repairs of incomplete package slots. They no longer fail with `failed to remove existing directory ... prior to swap: Directory not empty` or briefly remove a package directory another process is reading.
