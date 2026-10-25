---
"@pnpm/pnpr": patch
---

Team rosters, rule overrides, and upstream cache keys now always include the registry ecosystem (`<ecosystem>/<name>`). Moving a registry into an ecosystem group keeps its stored state and cache. Package tombstones now use the package directory layout. Internal storage and cache directories now use unversioned names (`.team-rosters`, `.rule-overrides`, `.scim-users`, `.pipeline-runs`, `.tombstones`, `.uploads`, `.journal`, `.staging`, `.artifacts`, `shared-artifacts`, `resolver-store`, and `resolver-cache`).
