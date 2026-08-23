---
"pacquet": patch
---

`@pnpm/exe` is no longer published from pnpm v12. Up to v11 it was the build of pnpm that bundled a Node.js runtime, so it could run where `pnpm` (a JavaScript package) could not. From v12 the `pnpm` package is itself the native executable and needs no Node.js, which left `@pnpm/exe` an identical copy of it. Install `pnpm` instead; the per-platform `@pnpm/exe.<target>` packages that carry the native binaries are unaffected, and `pnpm self-update` already moves an `@pnpm/exe` install onto `pnpm` when it upgrades to v12.
