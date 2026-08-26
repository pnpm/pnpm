---
"pacquet": patch
---

`pnpm fetch` now links each virtual-store package's dependency bins, so a dependency's lifecycle script can invoke a sibling dependency's bin. Previously a `postinstall` calling one — as `unrs-resolver` does with `napi-postinstall` — failed with `command not found` in the Docker "fetcher stage" shape (a lockfile with no project manifest), while `pnpm install` against the same lockfile succeeded [#14174](https://github.com/pnpm/pnpm/issues/14174).
