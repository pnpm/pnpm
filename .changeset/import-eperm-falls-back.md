---
"pacquet": patch
---

A hardlink or reflink refused with `EPERM` no longer fails the install under `packageImportMethod: auto` or `clone-or-copy`. An explicit `hardlink` copies the file instead. An explicit `clone` is unchanged. `EACCES` still fails the install. This fixes repeat installs on filesystems without hardlinks, such as EdenFS, where every `file:` dependency re-imported by a second install failed with `Operation not permitted`, and `pnpm deploy` inside rootless containers, where `FICLONE` is refused the same way [#14722](https://github.com/pnpm/pnpm/issues/14722).
