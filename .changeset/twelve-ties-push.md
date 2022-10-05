---
"@pnpm/default-reporter": patch
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

When a direct dependency fails to resolve, print the path to the project directory in the error message.
