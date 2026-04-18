---
"@pnpm/store.cafs": minor
"@pnpm/worker": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/installing.package-requester": minor
"pnpm": minor
---

Added LRU cache for parsed JSON to eliminate redundant package index parsing across the install pipeline. Same package.json is now parsed only once per install invocation, reducing CPU time on large installs.