---
"pacquet": minor
---

Added the commands the Rust CLI was still missing:

- `pnpm get <key>` and `pnpm set <key> <value>` — the top-level spellings of `pnpm config get` and `pnpm config set`.
- `pnpm store status` — reports the packages whose files no longer match the store they were expanded from, failing with `ERR_PNPM_MODIFIED_DEPENDENCY`; and `pnpm store add <pkg>...` — fetches packages into the store without writing a manifest, a lockfile, or `node_modules`. Both previously panicked.
- `pnpm env use --global <version>` and `pnpm env list [<selector>]`, the deprecated Node.js-only front end to `pnpm runtime`.
- `pnpm edit`, `pnpm profile`, `pnpm token`, and `pnpm xmas` now fail with `ERR_PNPM_NOT_IMPLEMENTED` pointing at the npm CLI, instead of being taken for a package script.
