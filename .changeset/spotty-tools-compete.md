---
"@pnpm/fs.packlist": patch
"pnpm": patch
---

After upgrading one of our dependencies, we started to sometimes have an error on publish. We have forked `@npmcli/arborist` to patch it with a fix [#7269](https://github.com/pnpm/pnpm/pull/7269).
