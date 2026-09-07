---
"pacquet": patch
---

Fixed pnpm failing to provision the version pinned in `packageManager` when the project sets `nodeLinker: hoisted` [#14595](https://github.com/pnpm/pnpm/issues/14595). The engine install used the project's linker, so the downloaded pnpm landed in a temporary directory that pnpm deleted before running it. Provisioning a managed Node.js, Deno, or Bun runtime no longer follows a `nodeLinker: hoisted` set in the global config either.
