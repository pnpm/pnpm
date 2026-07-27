---
"pacquet": patch
---

An aliased dependency of a protocol that resolves under its own package name — `jsr:` and the named registries — is recorded in the lockfile importer again. `"bar-from-jsr": "jsr:@pnpm-e2e/bar@1.0.0"` resolved and installed, but the importer stayed empty, so nothing reading direct dependencies out of the lockfile (`outdated`, `update`, `licenses`, dedupe, frozen-install verification) could see it [#13362](https://github.com/pnpm/pnpm/issues/13362).
