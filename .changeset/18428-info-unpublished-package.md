---
"pnpm": patch
---

`pnpm info` and `pnpm view` now report a fully unpublished package with `ERR_PNPM_PACKAGE_NOT_FOUND`. They used to fail with a metadata decoding error, which stopped tools such as Changesets from checking the rest of a release. `pnpm add --save-types` now skips a `@types` package that was fully unpublished [#18428](https://github.com/pnpm/pnpm/issues/18428).
