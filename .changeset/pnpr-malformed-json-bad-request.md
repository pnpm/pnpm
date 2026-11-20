---
"@pnpm/pnpr": patch
---

Malformed JSON request bodies sent to login, token creation, package publish, packument update, staged publish, and security audit endpoints now return `400 Bad Request`.

[Related task](https://github.com/zkochan/pnpm-tasks/issues/49).
