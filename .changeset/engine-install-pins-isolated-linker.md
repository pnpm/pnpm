---
"pacquet": patch
---

Fixed pnpm failing to provision the version pinned in `packageManager` when the project sets `nodeLinker: hoisted`. Provisioning a managed Node.js, Deno, or Bun runtime now ignores a `nodeLinker: hoisted` in the global config too [#14595](https://github.com/pnpm/pnpm/issues/14595).
