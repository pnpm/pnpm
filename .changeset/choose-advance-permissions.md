---
"pnpm": major
---

`pnpm approve <pkg>` now asks which permissions to grant when the package has no pending requests. In CI or without an interactive terminal, configure `permissions` or use `pnpm approve-builds` to grant build permission in advance [#18415](https://github.com/pnpm/pnpm/issues/18415).
