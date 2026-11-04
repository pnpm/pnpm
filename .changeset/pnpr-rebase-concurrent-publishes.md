---
"@pnpm/pnpr": patch
---

Instances sharing a libSQL metadata database no longer refuse one of two publishes to the same package that commit at the same moment. The second one rebases its versions and tags onto the first and commits. A publish refused at its commit no longer removes its archives on libSQL, because another instance may have committed the same archive.
