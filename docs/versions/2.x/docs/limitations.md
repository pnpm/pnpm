---
id: limitations
title: Limitations
original_id: limitations
---

1. `npm-shrinkwrap.json` and `package-lock.json` are ignored. Unlike pnpm, npm can install the
same `name@version` multiple times and with different sets of dependencies.
npm's shrinkwrap file is designed to reflect the `node_modules` layout created
by npm. pnpm cannot create a similar layout, so it cannot respect
npm's lockfile format. However, see [pnpm import](pnpm-import.md).
2. You can't publish npm modules with `bundleDependencies` managed by pnpm.
3. Binstubs (files in `node_modules/.bin`) are always shell files not
symlinks to JS files. The shell files are created to help pluggable CLI apps
in finding their plugins in the unusual `node_modules` structure. This is very
rarely an issue and if you expect the file to be a js file, just reference the
original file instead, as described in [#736](https://github.com/pnpm/pnpm/issues/736).
4. Node.js doesn't work with the [--preserve-symlinks](https://nodejs.org/api/cli.html#cli_preserve_symlinks) flag when executed in a project that uses pnpm.

Got an idea for workarounds for these issues? [Share them.](https://github.com/pnpm/pnpm/issues/new)
