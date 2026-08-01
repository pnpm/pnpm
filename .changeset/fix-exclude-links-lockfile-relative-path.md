---
"pacquet": patch
---

Fixed an issue where `excludeLinksFromLockfile` incorrectly remaps workspace-internal `link:` dependencies by resolving relative link targets against the importer's `project_dir` before performing the subdirectory containment check.
