---
"@pnpm/workspace.package-patterns": patch
"pnpm": patch
---

Ignore Windows absolute path patterns (such as drive-letter and leading backslash paths) when normalizing workspace package patterns.
