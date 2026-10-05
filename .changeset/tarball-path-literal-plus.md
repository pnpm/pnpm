---
"@pnpm/deps.path": patch
"pnpm": patch
"pacquet": patch
---

Fixed two URL or local path dependencies sharing a virtual store directory when one URL had `+`, `#`, `:`, or `?` where the other had `/`. Such dependencies, including git dependencies pinned with `#`, now get a hash suffix on their directory name.
