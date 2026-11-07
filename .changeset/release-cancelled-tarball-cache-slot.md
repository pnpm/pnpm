---
"pacquet": patch
---

Fixed `pnpm install` hanging when a concurrent tarball download or manifest read is cancelled while other tasks are waiting on the same archive [zkochan/pnpm-tasks#23](https://github.com/zkochan/pnpm-tasks/issues/23).
