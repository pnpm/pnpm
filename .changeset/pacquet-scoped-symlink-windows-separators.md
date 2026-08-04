---
"pacquet": patch
---

Fixed installs failing on Windows when a scoped dependency (`@scope/name`) had to be symlinked. Its `node_modules/@scope/name` link path was built by joining the whole alias as one segment, which left a `/` in the otherwise `\`-separated path; that forward slash reached `CreateSymbolicLinkW`, which rejects forward-slash paths with `ERROR_DIRECTORY` (os error 267). Paths are now rewritten to native separators before every filesystem call in the symlink writer.
