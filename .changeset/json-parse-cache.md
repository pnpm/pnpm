---
"@pnpm/store.cafs": patch
"@pnpm/worker": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/installing.package-requester": patch
"pnpm": patch
---

Added LRU cache for parsed JSON to eliminate redundant package index parsing across the install pipeline. Same package.json is now parsed only once per install invocation, reducing CPU time on large installs.