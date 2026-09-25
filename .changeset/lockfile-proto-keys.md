---
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

pnpm no longer reports `pnpm-lock.yaml` as broken when a project depends on a package named `constructor`. A `__proto__` key in the lockfile is now kept as a plain entry when pnpm reads or writes the lockfile. It no longer replaces the prototype of the objects pnpm builds from it [#11028](https://github.com/pnpm/pnpm/issues/11028).
