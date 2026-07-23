---
"pacquet": patch
---

Fixed installs failing on Windows when the global virtual store is enabled. The `<store>/links/<scope>/<name>/<version>/<hash>` slot path is formatted with `/` separators (it doubles as a cross-platform canonical id), and those forward slashes were reaching `CreateSymbolicLinkW`, which rejects forward-slash paths with `ERROR_DIRECTORY` (os error 267). The slot path is now expanded into native path components before any filesystem call.
