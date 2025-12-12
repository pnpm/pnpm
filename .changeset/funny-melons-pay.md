---
"@pnpm/fetch": patch
---

When the node-fetch request redirects an installation link and returns a relative path, URL parsing may fail [#10286](https://github.com/pnpm/pnpm/pull/10286).
