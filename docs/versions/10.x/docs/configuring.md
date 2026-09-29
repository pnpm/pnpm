---
id: configuring
title: Configuring
---

pnpm uses [npm's configuration] formats. Hence, you should set configuration
the same way you would for npm. For example,

```
pnpm config set store-dir /path/to/.pnpm-store
```

If no store is configured, then pnpm will automatically create a store on the
same drive. If you need pnpm to work across multiple hard drives or filesystems,
please read [the FAQ].

Furthermore, pnpm uses the same configuration that npm uses for doing
installations. If you have a private registry and npm is configured to work with
it, pnpm should be able to authorize requests as well, with no additional
configuration.

In addition to those options, pnpm also allows you to use all parameters that
are flags (for example `--filter` or `--workspace-concurrency`) as options:

```
workspace-concurrency = 1
filter = @my-scope/*
```

See the [`config` command] for more information.

[npm's configuration]: https://docs.npmjs.com/misc/config
[the FAQ]: ./faq.md#does-pnpm-work-across-multiple-drives-or-filesystems
[`config` command]: ./cli/config.md
