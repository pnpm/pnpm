---
"pacquet": patch
---

`pnpm update --latest` no longer corrupts dependencies the npm registry does not serve. A `runtime:` dependency (such as `"node": "runtime:26.5.0"`), a `git`/`github:` URL, or a remote tarball URL had the dependency's *name* looked up on the npm registry and its specifier overwritten with that unrelated package's version. Each dependency's own resolver now decides what its specifier should become, so these keep the form they were declared in.

`pnpm update --latest` also no longer rewrites `package.json` when a dependency is already at its latest version.
