---
"@pnpm/config": minor
"@pnpm/normalize-registries": minor
"@pnpm/npm-resolver": minor
"@pnpm/package-requester": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/exportable-manifest": minor
"@pnpm/default-resolver": minor
"@pnpm/resolver-base": minor
"@pnpm/store-controller-types": minor
"pnpm": minor
---

**Added support for installing JSR packages.** You can now install JSR packages using the following syntax:

```
pnpm add jsr:<pkg_name>
```

or with a version range:

```
pnpm add jsr:<pkg_name>@<range>
```

For example, running:

```
pnpm add jsr:@foo/bar
```

will add the following entry to your `package.json`:

```json
{
  "dependencies": {
    "@foo/bar": "jsr:^0.1.2"
  }
}
```

When publishing, this entry will be transformed into a format compatible with npm, older versions of Yarn, and previous pnpm versions:

```json
{
  "dependencies": {
    "@foo/bar": "npm:@jsr/foo__bar@^0.1.2"
  }
}
```

Related issue: [#8941](https://github.com/pnpm/pnpm/issues/8941).

Note: The `@jsr` scope defaults to <https://npm.jsr.io/> if the `@jsr:registry` setting is not defined.
