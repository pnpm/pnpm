---
"pacquet": patch
---

`pnpm install` now skips a Python project that has only a `requirements.txt` when its directory has a `.venv` that pnpm did not create, and prints a warning. The install failed for the whole workspace before.
