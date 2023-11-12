# @pnpm/fs.packlist

## 1.0.2

### Patch Changes

- 74432d605: Downgraded `npm-packlist` because the newer version significantly slows down the installation of local directory dependencies, making it unbearably slow.

  `npm-packlist` was upgraded in [this PR](https://github.com/pnpm/pnpm/pull/7250) to fix [#6997](https://github.com/pnpm/pnpm/issues/6997). We added our own file deduplication to fix the issue of duplicate file entries.

## 1.0.1

### Patch Changes

- c7f1359b6: After upgrading one of our dependencies, we started to sometimes have an error on publish. We have forked `@npmcli/arborist` to patch it with a fix [#7269](https://github.com/pnpm/pnpm/pull/7269).

## 1.0.0

### Major Changes

- 500363647: Initial release.
