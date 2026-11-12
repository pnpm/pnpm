---
"@pnpm/pnpr": patch
---

pnpr now rechecks package ownership when another registry replica changes a package or its removal record during a publish, dist-tag change, maintainer update, yank, or unpublish [zkochan/pnpm-tasks#96](https://github.com/zkochan/pnpm-tasks/issues/96).
