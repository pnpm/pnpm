---
"@pnpm/worker": patch
"pnpm": patch
---

pnpm now limits its default worker pool to the CPU quota available to the process, up to 16 workers [#17382](https://github.com/pnpm/pnpm/issues/17382).
