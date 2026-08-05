---
"pacquet": minor
---

Deprecated packages are reported during installation: a directly depended-on deprecated package gets an immediate warning, and deprecated subdependencies are summarized in a single `<N> deprecated subdependencies found` line. Versions matched by `pnpm.allowedDeprecatedVersions` are not warned about [#11633](https://github.com/pnpm/pnpm/issues/11633).
