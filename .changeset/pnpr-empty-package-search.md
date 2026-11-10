---
"@pnpm/pnpr": patch
---

Removing the last version of a hosted npm package now removes the package so packument reads return `404` and search omits it, while retaining its tombstone and unpublished version records. Hosted npm search results now project `publisher` to `{"username": ...}` without exposing account IDs, and always include a `maintainers` array [zkochan/pnpm-tasks#47](https://github.com/zkochan/pnpm-tasks/issues/47).
