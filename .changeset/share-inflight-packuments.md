---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Fixed an out-of-memory regression when workspace projects concurrently resolve a package with large registry metadata [pnpm/pnpm#13077](https://github.com/pnpm/pnpm/issues/13077).
