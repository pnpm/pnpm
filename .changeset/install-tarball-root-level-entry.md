---
"pacquet": patch
---

`pnpm install` no longer fails on a package tarball that carries a file at the archive root, such as the `._*` entries macOS `tar` adds. The file is installed at the root of the package [#14701](https://github.com/pnpm/pnpm/issues/14701).

A `file:` tarball packed without the usual `package/` directory is now recorded under the name and version from its own `package.json`. It was recorded under the alias the dependency was given, at version 0.0.0.
