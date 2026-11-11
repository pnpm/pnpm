---
"@pnpm/pnpr": patch
---

Removing the last version of a hosted npm package now removes the package so packument reads return `404` and search omits it. Hosted npm search results now always include a `maintainers` array. Hosted npm search results now project `publisher` to `{"username": ...}` without exposing internal account IDs [zkochan/pnpm-tasks#47](https://github.com/zkochan/pnpm-tasks/issues/47).
