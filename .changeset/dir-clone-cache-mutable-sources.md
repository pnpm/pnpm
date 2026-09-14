---
"pacquet": patch
---

Fixed macOS installs materializing a `file:` tarball or a git-hosted tarball dependency from a cached package directory. That cache is keyed by the integrity recorded in the lockfile, which pins neither a local tarball's bytes nor whether a git-hosted tarball's `prepare` script ran, so an install could lay out the contents of an earlier one.
