---
"pnpm": patch
---

An install from a project inside a workspace with a shared lockfile no longer prefixes its progress lines with the path to the workspace root. In CI and piped output, a large download now prints one line when it finishes, without the `0.00 B` line at its start.
