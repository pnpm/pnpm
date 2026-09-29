---
id: aliases
title: Aliases
---

Aliases let you install packages with custom names.

Let's assume you use `lodash` all over your project. There is a bug in `lodash`
that breaks your project. You have a fix but `lodash` won't merge it. Normally
you would either install `lodash` from your fork directly (as a git-hosted
dependency) or publish it with a different name. If you use the second solution
you have to replace all the requires in your project with the new dependency
name (`require('lodash')` => `require('awesome-lodash')`). With aliases, you
have a third option.

Publish a new package called `awesome-lodash` and install it using `lodash` as
its alias:

```
pnpm add lodash@npm:awesome-lodash
```

No changes in code are needed. All the requires of `lodash` will now resolve to
`awesome-lodash`.

An npm alias selector has this form:

```text
<alias>@npm:<package>
<alias>@npm:<package>@<version-or-tag>
```

The target package may be scoped. For example, this installs a specific version
of `@babel/core` under the local name `babel-core`:

```sh
pnpm add babel-core@npm:@babel/core@8.0.6
```

The alias is the dependency name written to `package.json` and used by your
code. The package after `npm:` is the package pnpm resolves. `npm:` here is the
package-alias protocol, not a [named registry](./package-sources.md#named-registries);
the target package still uses its default or scope-specific npm registry configuration.

Sometimes you'll want to use two different versions of a package in your
project. Easy:

```sh
pnpm add lodash1@npm:lodash@1
pnpm add lodash2@npm:lodash@2
```

Now you can require the first version of lodash via `require('lodash1')` and the
second via `require('lodash2')`.

This gets even more powerful when combined with hooks. Maybe you want to replace
`lodash` with `awesome-lodash` in all the packages in `node_modules`. You can
easily achieve that with the following `.pnpmfile.mjs`:

```js
function readPackage(pkg) {
  if (pkg.dependencies && pkg.dependencies.lodash) {
    pkg.dependencies.lodash = 'npm:awesome-lodash@^1.0.0'
  }
  return pkg
}

export const hooks = {
  readPackage
}
```
