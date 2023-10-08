---
"pnpm": minor
---

The list of packages that are allowed to run installation scripts now may be provided in a separate configuration file. The path to the file should be specified via the `pnpm.onlyBuiltDependenciesFile` field in `package.json`. For instance:

```json
{
  "dependencies": {
    "@my-org/policy": "1.0.0"
  }
  "pnpm": {
    "onlyBuiltDependenciesFile": "node_modules/@my-org/policy/allow-build.json"
  }
}
```

In the example above, the list is loaded from a dependency. The JSON file with the list should contain an array of package names. For instance:

```json
[
  "esbuild",
  "@reflink/reflink"
]
```

With the above list, only `esbuild` and `@reflink/reflink` will be allowed to run scripts during installation.

Related issue: [#7137](https://github.com/pnpm/pnpm/issues/7137).
