---
"pacquet": patch
---

Hardlinking from the store now stats each store file once per install. Since pnpm 12.8.0, a store file was stat'ed again for every package that links it. On an offline install of a large workspace, that added up to 15% system CPU [#17500](https://github.com/pnpm/pnpm/issues/17500).
