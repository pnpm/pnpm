---
"@pnpm/pnpr": patch
---

Dist-tag writes now reject names that parse as semver ranges and targets that are not published versions. Removing `latest` is rejected while published versions exist.
