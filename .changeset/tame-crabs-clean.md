---
"@pnpm/cafs": patch
---

The content-addressable store locker should be only created once per process. This fixes an issue that started happening after merging [#6817](https://github.com/pnpm/pnpm/pull/6817)
