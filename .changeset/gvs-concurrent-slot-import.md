---
"pacquet": patch
---

Concurrent installs sharing a global virtual store no longer fail with `failed to remove existing directory ... prior to swap: Directory not empty`, and no longer briefly remove a package directory another process is reading.
