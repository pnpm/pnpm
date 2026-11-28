---
"pnpm": patch
---

Fixed two URL dependencies that could share a virtual store directory. A URL with `(` or `)` could match one with `_`, and so could a URL dependency with peers. A URL ending in what looks like a hash suffix could match another URL's hashed directory name. Such dependencies now get a hash suffix on their directory name.
