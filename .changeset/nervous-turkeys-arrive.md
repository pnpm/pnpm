---
"@pnpm/resolve-dependencies": patch
---

Don't print an info message about linked dependencies if they are real linked dependencies specified via the `link:` protocol in `package.json`.
