---
"@pnpm/fs.packlist": patch
"pnpm": patch
---

Downgraded `npm-packlist` because the newer version significantly slows down the installation of local directory dependencies, making it unbearably slow.

`npm-packlist` was upgraded in [this PR](https://github.com/pnpm/pnpm/pull/7250) to fix [#6997](https://github.com/pnpm/pnpm/issues/6997). We added our own file deduplication to fix the issue of duplicate file entries.
