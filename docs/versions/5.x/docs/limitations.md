---
id: limitations
title: Limitations
---

1. `npm-shrinkwrap.json` and `package-lock.json` are ignored. Unlike pnpm, npm
can install the same `name@version` multiple times and with different sets of
dependencies. npm's lockfile is designed to reflect the flat `node_modules`
layout, however, as pnpm cannot create a similar layout, it cannot respect
npm's lockfile format. See [pnpm import] if you wish to convert a lockfile to
pnpm's format, though.
2. pnpm can't publish npm packages with `bundledDependencies`. Currently, there
are no plans to add support for `bundledDependencies` either, as they do not
work very consistently in publishing, even on npm. Instead, we recommend you use
an actual package bundler, like webpack, rollup, or ESBuild.
3. Binstubs (files in `node_modules/.bin`) are always shell files, not
symlinks to JS files. The shell files are created to help pluggable CLI apps
in finding their plugins in the unusual `node_modules` structure. This is very
rarely an issue and if you expect the file to be a JS file, reference the
original file directly instead, as described in [#736].
4. Node's [--preserve-symlinks] flag does not work when executed in a project
that uses pnpm.

Got an idea for workarounds for these issues?
[Share them.](https://github.com/pnpm/pnpm/issues/new)

[pnpm import]: cli/import.md
[#736]: https://github.com/pnpm/pnpm/issues/736
[--preserve-symlinks]: https://nodejs.org/api/cli.html#cli_preserve_symlinks
