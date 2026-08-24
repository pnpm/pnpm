---
"pacquet": patch
---

`@pnpm/exe` is no longer published from pnpm v12. It existed to run pnpm without a Node.js runtime; the `pnpm` package is now itself the native executable, so install `pnpm` instead. An existing `@pnpm/exe` install needs no action — `pnpm self-update` moves it onto `pnpm` when it upgrades to v12.
