---
id: configuring
title: Configuring
original_id: configuring
---

pnpm uses [npm's configs](https://docs.npmjs.com/misc/config). Hence, you should set configs the same way you would for npm. For example,

```
npm config set store-dir /path/to/.pnpm-store
```

If no store is configured, then pnpm will automatically create a store on the same drive.
If configuring pnpm to work across multiple hard drives or filesystems, please read [the FAQ](faq.md#does-pnpm-work-across-multiple-hard-drives-or-filesystems).

Furthermore, pnpm uses the same configs that npm uses for doing installations. If you have a private registry and npm is configured
to work with it, pnpm should be able to authorize requests as well, with no additional configuration.

In addition to those options, pnpm also allows you to use all parameters that are flags (for example `--filter` or `--workspace-concurrency`) as option:
```
workspace-concurrency = 1
filter = @my-scope/*
```
