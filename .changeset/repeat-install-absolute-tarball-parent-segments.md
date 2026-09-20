---
"pacquet": patch
---

Repeat installs are now up to date when a dependency's `file:` specifier is an absolute path containing `..`. Such a path never matched the one recorded in the lockfile, so every install reinstalled the dependency.
