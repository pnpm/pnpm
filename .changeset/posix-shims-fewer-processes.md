---
"pacquet": patch
---

POSIX bin shims start fewer processes. Running a dependency's bin through its shim from Git Bash on Windows starts 7 processes where it started 15. Installing again replaces the shims already in `node_modules`.
