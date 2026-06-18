---
"@pnpm/resolving.npm-resolver": minor
"@pnpm/config.reader": minor
"@pnpm/types": minor
"@pnpm/store.connection-manager": patch
"pnpm": minor
---

Added a new setting `nativeBinDependencies`. Packages listed in it (along with the built-in defaults `pacquet` and `@pnpm/pacquet`) that ship a JavaScript launcher plus per-platform native binaries as `optionalDependencies` are now installed by fetching only the matching platform's binary and linking it directly into `node_modules/.bin`. This skips the launcher shim (no extra Node.js process per invocation) and the package's lifecycle scripts, and avoids downloading the other platforms' binaries.
