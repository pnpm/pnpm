---
"pacquet": patch
---

`pnpm update --latest` now keeps dependencies that the npm registry does not serve in the form they were declared. A `runtime:` dependency (such as `"node": "runtime:26.5.0"`), a `git`/`github:` URL, or a remote tarball URL previously had its *name* looked up on the npm registry and its specifier overwritten with that unrelated package's version.

`pnpm update --latest` also no longer rewrites `package.json` when a dependency is already at its latest version.
