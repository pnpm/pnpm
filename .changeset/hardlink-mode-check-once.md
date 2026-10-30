---
"pacquet": patch
---

Hardlinking from the store stats each store file once per install, not once per package that links it. An offline install of a large workspace made about twice as many `stat` calls as before and spent up to 15% more system CPU [#17500](https://github.com/pnpm/pnpm/issues/17500).
