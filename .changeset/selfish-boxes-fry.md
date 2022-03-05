---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

The location of an injected directory dependency should be correctly located, when there is a chain of local dependencies (declared via the `file:` protocol`).

The next scenario was not working prior to the fix. There are 3 projects in the same folder: foo, bar, qar.

`foo/package.json`:

```json
{
  "name": "foo",
  "dependencies": {
    "bar": "file:../bar"
  },
  "dependenciesMeta": {
    "bar": {
      "injected": true
    }
  }
}
```

`bar/package.json`:

```json
{
  "name": "bar",
  "dependencies": {
    "qar": "file:../qar"
  },
  "dependenciesMeta": {
    "qar": {
      "injected": true
    }
  }
}
```

`qar/package.json`:

```json
{
  "name": "qar"
}
```

Related PR: [#4415](https://github.com/pnpm/pnpm/pull/4415).
