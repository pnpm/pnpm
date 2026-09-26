---
"@pnpm/pnpr": patch
---

A publish whose file another writer had already stored under the same name now answers `409` instead of `201`. The version it described was left out of the registry document, so the publish did not happen. This can only occur where several pnpr instances share one object store.
