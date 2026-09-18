## 0.1.0-alpha.12

pnpr 0.1.0-alpha.12 answers broad searches against an upstream registry instead of failing, and keeps Python resolution going when an index or a release carries metadata it cannot read.

### Patch Changes

- `pnpm search` and `npm search` against an upstream with `search: true` no longer fail with a 400 error on broad terms. pnpr now truncates an upstream that holds more results than its fetch budget downloads. The results it could not download keep `total` approximate.

- Python resolution no longer fails on a release whose `Requires-Python` is not a version specifier, such as the trailing comma in `openpyxl` 3.0.x. pnpm now reads such a value as if the release declared no interpreter range [#14910](https://github.com/pnpm/pnpm/issues/14910).

- A Python release whose wheel metadata declares a requirement pnpm cannot read no longer fails the install. pnpm now resolves the project against the other releases of that package, and reports the unreadable requirement when none of them works.

- `pnpm install` no longer fails when a Python index lists a file pnpm cannot use, such as a release with no SHA-256 digest or an unreadable wheel filename. That file is left out and the project resolves against the remaining releases.

- `pnpm install` now includes dependencies referenced by weak Cargo features in `Cargo.lock`. Cargo no longer rejects the generated lockfile with `--locked` for crates such as `uuid` [#14978](https://github.com/pnpm/pnpm/issues/14978).
