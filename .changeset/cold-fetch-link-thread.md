---
"pacquet": patch
---

Sped up cold installs and `pnpm fetch` on macOS. pnpm now links downloaded packages into the virtual store on a separate thread, so a slow link pass no longer pauses the downloads that are still in flight [#17271](https://github.com/pnpm/pnpm/issues/17271).
